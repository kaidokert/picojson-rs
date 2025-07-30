// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for StreamParser using StreamBuffer.

use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::ContentExtractor;
use crate::shared::{ContentRange, DataSource, State};
use crate::stream_buffer::StreamBuffer;
use crate::stream_parser::Reader;
use crate::{Event, JsonNumber, ParseError};

/// ContentBuilder implementation for StreamParser that uses StreamBuffer for streaming and escape processing
pub struct StreamContentBuilder<'b, R: Reader> {
    /// StreamBuffer for single-buffer input and escape processing
    stream_buffer: StreamBuffer<'b>,
    /// The reader for fetching more data
    reader: R,
    /// Parser state tracking
    parser_state: State,
    /// Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
    /// Flag to reset unescaped content on next operation
    unescaped_reset_queued: bool,
    /// Flag to track when the input stream has been finished (for number parsing)
    finished: bool,
}

impl<'b, R: Reader> StreamContentBuilder<'b, R> {
    /// Create a new StreamContentBuilder
    pub fn new(buffer: &'b mut [u8], reader: R) -> Self {
        Self {
            stream_buffer: StreamBuffer::new(buffer),
            reader,
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            unescaped_reset_queued: false,
            finished: false,
        }
    }

    /// Fill the buffer from the reader
    fn fill_buffer_from_reader(&mut self) -> Result<(), ParseError> {
        // If buffer is full, try to compact it first (original compaction logic)
        if self.stream_buffer.get_fill_slice().is_none() {
            // Buffer is full - ALWAYS attempt compaction
            let compact_start_pos = match self.parser_state {
                State::Number(start_pos) => start_pos,
                State::Key(start_pos) => start_pos,
                State::String(start_pos) => start_pos,
                _ => self.stream_buffer.current_position(),
            };

            let compaction_offset = self
                .stream_buffer
                .compact_from(compact_start_pos)
                .map_err(ParseError::from)?;

            if compaction_offset == 0 {
                // Buffer too small for current token - this is an input buffer size issue
                return Err(ParseError::InputBufferFull);
            }

            // Update parser state positions after compaction (original logic)
            self.update_positions_after_compaction(compaction_offset)?;
        }

        if let Some(fill_slice) = self.stream_buffer.get_fill_slice() {
            let bytes_read = self
                .reader
                .read(fill_slice)
                .map_err(|_| ParseError::ReaderError)?;
            self.stream_buffer
                .mark_filled(bytes_read)
                .map_err(ParseError::from)?;
        }
        Ok(())
    }

    /// Update parser state positions after compaction (original logic)
    fn update_positions_after_compaction(
        &mut self,
        compaction_offset: usize,
    ) -> Result<(), ParseError> {
        // Update positions - since we compact from the token start position,
        // positions should not be discarded in normal operation
        match &mut self.parser_state {
            State::None => {
                // No position-based state to update
            }
            State::Key(pos) | State::String(pos) | State::Number(pos) => {
                if *pos >= compaction_offset {
                    *pos = pos.checked_sub(compaction_offset).unwrap_or(0);
                } else {
                    return Err(ParseError::Unexpected(
                        crate::shared::UnexpectedState::InvalidSliceBounds,
                    ));
                }
            }
        }
        Ok(())
    }

    /// Set the finished state (called by StreamParser when input is exhausted)
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Apply queued unescaped content reset if flag is set
    pub fn apply_unescaped_reset_if_queued(&mut self) {
        if self.unescaped_reset_queued {
            self.stream_buffer.clear_unescaped();
            self.unescaped_reset_queued = false;
        }
    }

    /// Queue a reset of unescaped content for the next operation
    fn queue_unescaped_reset(&mut self) {
        self.unescaped_reset_queued = true;
    }


    /// Start escape processing using StreamBuffer
    fn start_escape_processing(&mut self) -> Result<(), ParseError> {
        // Initialize escape processing with StreamBuffer if not already started
        if !self.stream_buffer.has_unescaped_content() {
            if let State::String(start_pos) | State::Key(start_pos) = self.parser_state {
                let current_pos = self.stream_buffer.current_position();

                // start_pos already points to content start position (not quote position)
                let content_start = start_pos;
                // Content to copy ends right before the escape character
                let content_end = if self.unicode_escape_collector.has_pending_high_surrogate() {
                    // Skip copying high surrogate text when processing low surrogate
                    content_start
                } else {
                    ContentRange::end_position_excluding_delimiter(current_pos)
                };

                // Estimate max length needed for unescaping (content so far + remaining buffer)
                let content_len = content_end.wrapping_sub(content_start);
                let max_escaped_len = self
                    .stream_buffer
                    .remaining_bytes()
                    .checked_add(content_len)
                    .ok_or(ParseError::NumericOverflow)?;

                // Start unescaping with StreamBuffer and copy existing content
                self.stream_buffer.start_unescaping_with_copy(
                    max_escaped_len,
                    content_start,
                    content_end,
                )?;
            }
        }

        Ok(())
    }
}

impl<R: Reader> ContentExtractor for StreamContentBuilder<'_, R> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        // If buffer is empty, try to fill it first
        if self.stream_buffer.is_empty() {
            self.fill_buffer_from_reader()?;
        }

        // If still empty after fill attempt, we're at EOF
        if self.stream_buffer.is_empty() {
            if !self.finished {
                self.finished = true;
            }
            return Ok(None);
        }

        // Get byte and advance
        let byte = self.stream_buffer.current_byte()?;
        self.stream_buffer.advance()?;

        Ok(Some(byte))
    }

    fn parser_state_mut(&mut self) -> &mut State {
        &mut self.parser_state
    }

    fn current_position(&self) -> usize {
        self.stream_buffer.current_position()
    }

    fn begin_string_content(&mut self, _pos: usize) {
        // StreamParser doesn't need explicit string begin processing
        // as it handles content accumulation automatically
    }

    fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
        &mut self.unicode_escape_collector
    }

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // StreamParser-specific: Queue reset to prevent content contamination
        if self.has_unescaped_content() {
            self.queue_unescaped_reset();
        }
        let current_pos = self.current_position();
        let content_piece = crate::shared::get_content_piece(self, start_pos, current_pos)?;
        Ok(Event::String(content_piece.to_string()?))
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // StreamParser-specific: Queue reset to prevent content contamination
        if self.has_unescaped_content() {
            self.queue_unescaped_reset();
        }
        let current_pos = self.current_position();
        let content_piece = crate::shared::get_content_piece(self, start_pos, current_pos)?;
        Ok(Event::Key(content_piece.to_string()?))
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // Use shared number parsing with StreamParser-specific document end detection
        // StreamParser uses state-based detection: finished flag indicates true document end
        let current_pos = self.stream_buffer.current_position();

        // A standalone number at the end of the document has no trailing delimiter, so we use the full span.
        let use_full_span = !from_container_end && finished;
        let end_pos = ContentRange::number_end_position(current_pos, use_full_span);

        let number_bytes = self
            .stream_buffer
            .get_string_slice(start_pos, end_pos)
            .map_err(ParseError::from)?;
        let json_number = JsonNumber::from_slice(number_bytes)?;
        Ok(Event::Number(json_number))
    }

    fn validate_and_extract_number(
        &mut self,
        from_container_end: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        let start_pos = match *self.parser_state() {
            State::Number(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        *self.parser_state_mut() = State::None;
        self.extract_number(start_pos, from_container_end, self.is_finished())
    }

    fn parser_state(&self) -> &State {
        &self.parser_state
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        // Called when Begin(UnicodeEscape) is received
        Ok(())
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Define the provider for getting hex digits from the stream buffer
        let hex_slice_provider = |start, end| {
            self.stream_buffer
                .get_string_slice(start, end)
                .map_err(Into::into)
        };

        // Call the shared processor, which now returns the result by value
        let (utf8_bytes_result, _) = crate::escape_processor::process_unicode_escape_sequence(
            self.stream_buffer.current_position(),
            &mut self.unicode_escape_collector,
            hex_slice_provider,
        )?;

        // Handle the UTF-8 bytes if we have them
        if let Some((utf8_bytes, len)) = utf8_bytes_result {
            // Append the resulting bytes to the unescaped buffer
            for &byte in &utf8_bytes[..len] {
                self.stream_buffer
                    .append_unescaped_byte(byte)
                    .map_err(ParseError::from)?;
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        self.start_escape_processing()
    }
}

impl<R: Reader> StreamContentBuilder<'_, R> {
    /// Handle byte accumulation for StreamParser-specific requirements
    /// This method is called when a byte doesn't generate any events
    pub fn handle_byte_accumulation(&mut self, byte: u8) -> Result<(), ParseError> {
        // Check if we're in a string or key state and should accumulate bytes
        let in_string_mode = matches!(self.parser_state, State::String(_) | State::Key(_));

        if in_string_mode {
            // When unescaped content is active, we need to accumulate ALL string content.
            // The ParserCore now correctly prevents this function from being called for
            // bytes that are part of an escape sequence.
            if self.stream_buffer.has_unescaped_content() {
                self.stream_buffer
                    .append_unescaped_byte(byte)
                    .map_err(ParseError::from)?;
            }
        }
        Ok(())
    }
}

/// DataSource implementation for StreamContentBuilder
/// 
/// This implementation provides access to both borrowed content from the StreamBuffer's
/// internal buffer and unescaped content from the StreamBuffer's scratch space.
/// Note: StreamParser doesn't have a distinct 'input lifetime since it reads from a stream,
/// so we use the buffer lifetime 'b for both borrowed and unescaped content.
impl<'b, R: Reader> DataSource<'b, 'b> for StreamContentBuilder<'b, R> {
    fn get_borrowed_slice(&'b self, start: usize, end: usize) -> Result<&'b [u8], ParseError> {
        self.stream_buffer.get_string_slice(start, end).map_err(Into::into)
    }

    fn get_unescaped_slice(&'b self) -> Result<&'b [u8], ParseError> {
        self.stream_buffer.get_unescaped_slice().map_err(Into::into)
    }

    fn has_unescaped_content(&self) -> bool {
        self.stream_buffer.has_unescaped_content()
    }
}
