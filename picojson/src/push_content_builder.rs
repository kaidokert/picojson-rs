// SPDX-License-Identifier: Apache-2.0

//! Content extractor for PushParser.

use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::ContentExtractor;
use crate::shared::{DataSource, State};
use crate::stream_buffer::StreamBuffer;
use crate::{Event, JsonNumber, ParseError};

/// A trait for handling events from a SAX-style push parser.
///
/// # Generic Parameters
///
/// * `'input` - Lifetime for the input data being parsed
/// * `'scratch` - Lifetime for the scratch buffer used for temporary storage
/// * `E` - The error type that can be returned by the handler
pub trait PushParserHandler<'input, 'scratch, E> {
    /// Handles a single, complete JSON event.
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), E>;
}

/// Content extractor for PushParser.
pub struct PushContentBuilder<'input, 'scratch> {
    /// StreamBuffer for single-buffer input and escape processing
    stream_buffer: StreamBuffer<'scratch>,
    /// Parser state tracking
    parser_state: State,
    /// Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
    /// Flag to reset unescaped content on next operation
    unescaped_reset_queued: bool,
    /// Position offset for tracking absolute positions across chunks
    position_offset: usize,
    /// Current position within the current chunk
    current_position: usize,
    /// Position where the current token started
    token_start_pos: usize,
    /// Whether we're using the unescaped buffer for current content
    using_unescaped_buffer: bool,
    /// The current chunk of data being processed
    current_chunk: &'input [u8],
    /// The cursor for the current chunk
    chunk_cursor: usize,
    /// Whether we're currently collecting Unicode escape hex digits
    in_unicode_escape: bool,
    /// Whether we're currently processing a simple escape sequence
    in_simple_escape: bool,
}

impl<'input, 'scratch> PushContentBuilder<'input, 'scratch> {
    /// Create a new PushContentBuilder
    pub fn new(buffer: &'scratch mut [u8]) -> Self {
        Self {
            stream_buffer: StreamBuffer::new(buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            unescaped_reset_queued: false,
            position_offset: 0,
            current_position: 0,
            token_start_pos: 0,
            using_unescaped_buffer: false,
            current_chunk: &[],
            chunk_cursor: 0,
            in_unicode_escape: false,
            in_simple_escape: false,
        }
    }

    /// Set the current chunk of data to be processed
    pub fn set_chunk(&mut self, chunk: &'input [u8]) {
        self.current_chunk = chunk;
        self.chunk_cursor = 0;
    }

    /// Reset input processing state
    pub fn reset_input(&mut self) {
        self.current_chunk = &[];
        self.chunk_cursor = 0;
    }

    /// Update position offset by adding to it
    pub fn add_position_offset(&mut self, amount: usize) {
        self.position_offset += amount;
    }

    /// Append a byte to the unescaped buffer
    pub fn append_unescaped_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(byte)
            .map_err(ParseError::from)
    }

    /// Apply queued unescaped content reset if needed
    pub fn apply_unescaped_reset_if_queued(&mut self) {
        if self.unescaped_reset_queued {
            self.unescaped_reset_queued = false;
            self.using_unescaped_buffer = false; // Always reset the flag when buffer is cleared
            self.stream_buffer.clear_unescaped();
        }
    }

    /// Queue a reset of unescaped content for the next operation
    fn queue_unescaped_reset(&mut self) {
        self.unescaped_reset_queued = true;
    }

    /// Handle byte accumulation with selective logic based on current state
    pub fn handle_byte_accumulation(&mut self, byte: u8) -> Result<(), ParseError> {
        // Check if we're currently processing any type of escape sequence
        if self.in_unicode_escape {
            // During Unicode escape processing, try to feed hex digits directly to the collector
            if crate::escape_processor::EscapeProcessor::validate_hex_digit(byte).is_ok() {
                let is_complete = self.unicode_escape_collector.add_hex_digit(byte)?;
                if is_complete {
                    // Process the complete escape sequence immediately
                    let mut utf8_buffer = [0u8; 4];
                    let (utf8_bytes_opt, _surrogate_state_changed) = self
                        .unicode_escape_collector
                        .process_to_utf8(&mut utf8_buffer)?;

                    if let Some(utf8_bytes) = utf8_bytes_opt {
                        // Write the UTF-8 bytes directly to the scratch buffer
                        for &utf8_byte in utf8_bytes {
                            self.stream_buffer
                                .append_unescaped_byte(utf8_byte)
                                .map_err(ParseError::from)?;
                        }
                    }
                    // Reset collector and exit Unicode escape mode
                    self.unicode_escape_collector.reset();
                    self.in_unicode_escape = false;
                }
                return Ok(());
            } else {
                // Non-hex digit during Unicode escape - this shouldn't happen in valid JSON
                self.in_unicode_escape = false;
            }
        } else if self.in_simple_escape {
            // This is a Unicode escape - do NOT accumulate the 'u', let the escape processor handle it
            self.in_simple_escape = false;
            return Ok(()); // Skip accumulation for 'u' in Unicode escapes
        }

        // Regular byte accumulation logic for non-hex digits or when not in Unicode escape
        let should_accumulate = match self.parser_state {
            State::String(_) | State::Key(_) => {
                // We're in string/key context - accumulate if using unescaped buffer
                // BUT: skip accumulation of escape characters when in Unicode escape mode
                // OR when we encounter a backslash (which will be handled by escape processor)
                if self.in_unicode_escape || self.in_simple_escape {
                    // Don't accumulate escape characters - they're handled by escape processors
                    false
                } else if byte == b'\\' {
                    // Don't accumulate backslashes - they trigger escape processing
                    false
                } else if byte == b'"' {
                    // Don't accumulate closing quotes - they mark end of string
                    false
                } else {
                    self.has_unescaped_content()
                }
            }
            State::Number(_) => {
                // We're in number context - accumulate if using unescaped buffer (for numbers spanning chunks)
                self.has_unescaped_content()
            }
            _ => false, // Not in string/key/number context - don't accumulate
        };

        if should_accumulate {
            self.append_unescaped_byte(byte)?;
        }

        Ok(())
    }
}

impl ContentExtractor for PushContentBuilder<'_, '_> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        if self.chunk_cursor < self.current_chunk.len() {
            let byte = self.current_chunk[self.chunk_cursor];
            self.chunk_cursor += 1;
            self.current_position = self.position_offset + self.chunk_cursor - 1;
            Ok(Some(byte))
        } else {
            Ok(None)
        }
    }

    fn parser_state_mut(&mut self) -> &mut State {
        &mut self.parser_state
    }

    fn parser_state(&self) -> &State {
        &self.parser_state
    }

    fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
        &mut self.unicode_escape_collector
    }

    fn current_position(&self) -> usize {
        self.current_position
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.token_start_pos = pos;
        self.stream_buffer.clear_unescaped();
    }

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // Queue reset if using unescaped content (same as the old manual path)
        if self.has_unescaped_content() {
            self.queue_unescaped_reset();
        }

        // PushParser: current_position points AT the closing quote, but get_content_piece expects
        // position AFTER the closing quote, so add 1
        let content_piece =
            crate::shared::get_content_piece(self, start_pos + 1, self.current_position + 1)?;
        content_piece.into_string().map(Event::String)
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // Queue reset if using unescaped content (same as the old manual path)
        if self.has_unescaped_content() {
            self.queue_unescaped_reset();
        }

        // The entire token was contained in the current chunk - use direct extraction
        let content_piece =
            crate::shared::get_content_piece(self, start_pos + 1, self.current_position + 1)?;
        content_piece.into_string().map(Event::Key)
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        _from_container_end: bool,
        _finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // Queue reset if using unescaped content (same as the old manual path)
        if self.has_unescaped_content() {
            self.queue_unescaped_reset();
        }

        let content_piece =
            crate::shared::get_content_piece(self, start_pos + 1, self.current_position + 1)?;
        let number_bytes = content_piece.as_bytes();
        let json_number = JsonNumber::from_slice(number_bytes)?;
        Ok(Event::Number(json_number))
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        // Implement copy-on-escape: copy the clean part before the escape to unescaped buffer
        if !self.has_unescaped_content() {
            if let State::String(start_pos) | State::Key(start_pos) = self.parser_state {
                // start_pos points to the opening quote, so content starts at start_pos + 1
                // Current position is where the escape character (\) is located
                // We want to copy content up to (but not including) the escape character
                // Copy the clean part to the unescaped buffer
                self.copy_content_chunk_to_scratch(start_pos + 1, self.current_position)?;

                // Mark that we're now using the unescaped buffer
                self.using_unescaped_buffer = true;
            }
        }

        // Set a general escape flag to skip the next byte (which will be the escape character)
        // This will be overridden if begin_unicode_escape is called
        self.in_simple_escape = true;
        self.in_unicode_escape = false;
        Ok(())
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        // Start of unicode escape sequence - reset collector for new sequence and enter escape mode
        // Note: we preserve pending high surrogate state for surrogate pair processing
        self.unicode_escape_collector.reset();
        self.in_unicode_escape = true;
        self.in_simple_escape = false; // Override the simple escape flag set by begin_escape_sequence

        // CRITICAL: The tokenizer processes \u and the first hex digit before emitting Begin(UnicodeEscape)
        // Since we no longer accumulate the 'u' character, we only need to handle the first hex digit
        // that was accumulated before this event arrived
        if self.has_unescaped_content() {
            // Get current buffer content and check if it ends with a hex digit (the first one)
            if let Ok(current_content) = self.stream_buffer.get_unescaped_slice() {
                if !current_content.is_empty() {
                    let hex_pos = current_content.len() - 1;

                    if crate::escape_processor::EscapeProcessor::validate_hex_digit(
                        current_content[hex_pos],
                    )
                    .is_ok()
                    {
                        let first_hex_digit = current_content[hex_pos];

                        // Remove the last hex digit by truncating the buffer
                        self.stream_buffer.truncate_unescaped_by(1);

                        // Now feed the first hex digit to the Unicode collector
                        let is_complete = self
                            .unicode_escape_collector
                            .add_hex_digit(first_hex_digit)?;
                        if is_complete {
                            // This shouldn't happen for the first hex digit, but handle it just in case
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        // Now we know this is definitely a simple escape, not Unicode
        self.in_simple_escape = false; // Reset flag since we're processing it now

        if self.has_unescaped_content() {
            self.stream_buffer
                .append_unescaped_byte(escape_char)
                .map_err(ParseError::from)
        } else {
            // This shouldn't happen if begin_escape_sequence was called properly
            Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ))
        }
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // With the selective accumulation approach, Unicode escape processing should have
        // already happened during byte accumulation via handle_byte_accumulation().
        // This method is called at the end of a Unicode escape sequence by the event processor.
        // If the collector still has incomplete data, it means we're dealing with chunked input
        // where hex digits span chunk boundaries, OR we have a bug where hex digits aren't
        // being fed properly.
        Ok(())
    }
}

impl PushContentBuilder<'_, '_> {
    /// Copy content from current chunk to scratch buffer based on current parser state
    fn copy_content_chunk_to_scratch(
        &mut self,
        content_start: usize,
        content_end: usize,
    ) -> Result<(), ParseError> {
        if content_end > content_start {
            // Convert absolute positions to relative positions within the current data chunk
            let slice_start = content_start.saturating_sub(self.position_offset);
            let slice_end = content_end.saturating_sub(self.position_offset);

            if slice_end <= self.current_chunk.len() && slice_start < slice_end {
                let partial_slice = &self.current_chunk[slice_start..slice_end];

                for &byte in partial_slice {
                    self.stream_buffer.append_unescaped_byte(byte)?;
                }
            }
        }
        Ok(())
    }

    /// Copy partial content from current chunk to scratch buffer when chunk boundary reached
    pub fn copy_partial_content_to_scratch(&mut self) -> Result<(), ParseError> {
        // Determine the start of the current token content based on parser state
        let content_start = match self.parser_state {
            State::String(start_pos) | State::Key(start_pos) => {
                // For strings and keys, content starts after the opening quote
                start_pos + 1
            }
            State::Number(start_pos) => {
                // For numbers, start_pos points to the character before the first digit
                // so we need to add 1 to get to the actual number content
                start_pos + 1
            }
            _ => {
                return Ok(());
            }
        };

        // The end is the current position (where we are in the chunk)
        // Copy the slice of partial content from the current chunk using the common method
        self.copy_content_chunk_to_scratch(content_start, self.current_position + 1)
    }
}

impl<'input, 'scratch> DataSource<'input, 'scratch> for PushContentBuilder<'input, 'scratch> {
    fn get_borrowed_slice(
        &'input self,
        start: usize,
        end: usize,
    ) -> Result<&'input [u8], ParseError> {
        // For now, always try to read from current input chunk regardless of escape mode
        // The issue was that process_unicode_escape_sequence calls this directly to get hex digits
        // But for PushParser, hex digits might not be in the current chunk due to chunked processing

        // Convert absolute positions to relative positions within the current data chunk
        let slice_start = start.saturating_sub(self.position_offset);
        let slice_end = end.saturating_sub(self.position_offset);

        // Check if the requested range is within the current chunk
        if slice_end > self.current_chunk.len() || slice_start > slice_end {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }

        let result = &self.current_chunk[slice_start..slice_end];
        Ok(result)
    }

    fn get_unescaped_slice(&'scratch self) -> Result<&'scratch [u8], ParseError> {
        self.stream_buffer
            .get_unescaped_slice()
            .map_err(ParseError::from)
    }

    fn has_unescaped_content(&self) -> bool {
        self.stream_buffer.has_unescaped_content() || self.using_unescaped_buffer
    }
}
