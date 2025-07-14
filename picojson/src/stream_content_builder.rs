// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for StreamParser using StreamBuffer.

use crate::content_builder::ContentBuilder;
use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::{ContentExtractor, EscapeHandler};
use crate::shared::{ContentRange, State};
use crate::stream_buffer::StreamBuffer;
use crate::{Event, ParseError, String};

/// ContentBuilder implementation for StreamParser that uses StreamBuffer for streaming and escape processing
pub struct StreamContentBuilder<'b> {
    /// StreamBuffer for single-buffer input and escape processing
    stream_buffer: StreamBuffer<'b>,
    /// Parser state tracking
    parser_state: State,
    /// Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
    /// Flag to reset unescaped content on next operation
    unescaped_reset_queued: bool,
    /// Flag to track when we're inside a Unicode escape sequence (collecting hex digits)
    in_unicode_escape: bool,
}

impl<'b> StreamContentBuilder<'b> {
    /// Create a new StreamContentBuilder
    pub fn new(buffer: &'b mut [u8]) -> Self {
        Self {
            stream_buffer: StreamBuffer::new(buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            unescaped_reset_queued: false,
            in_unicode_escape: false,
        }
    }

    /// Get access to the stream buffer for byte operations
    pub fn stream_buffer(&self) -> &StreamBuffer<'b> {
        &self.stream_buffer
    }

    /// Get mutable access to the stream buffer for byte operations
    pub fn stream_buffer_mut(&mut self) -> &mut StreamBuffer<'b> {
        &mut self.stream_buffer
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

    /// Helper to create an unescaped string from StreamBuffer
    fn create_unescaped_string(&mut self) -> Result<String<'_, '_>, ParseError> {
        self.queue_unescaped_reset();
        let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
        let str_content = crate::shared::from_utf8(unescaped_slice)?;
        Ok(String::Unescaped(str_content))
    }

    /// Helper to create a borrowed string from StreamBuffer
    fn create_borrowed_string(
        &mut self,
        content_start: usize,
    ) -> Result<String<'_, '_>, ParseError> {
        let current_pos = self.stream_buffer.current_position();
        let (content_start, content_end) =
            ContentRange::string_content_bounds_from_content_start(content_start, current_pos);

        let bytes = self
            .stream_buffer
            .get_string_slice(content_start, content_end)?;
        let str_content = crate::shared::from_utf8(bytes)?;
        Ok(String::Borrowed(str_content))
    }

    /// Start escape processing using StreamBuffer
    fn start_escape_processing(&mut self) -> Result<(), ParseError> {
        // Initialize escape processing with StreamBuffer if not already started
        if !self.stream_buffer.has_unescaped_content() {
            if let State::String(start_pos) | State::Key(start_pos) = self.parser_state {
                let current_pos = self.stream_buffer.current_position();

                log::debug!(
                    "[NEW] start_escape_processing: start_pos={}, current_pos={}",
                    start_pos,
                    current_pos
                );

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

                log::debug!("[NEW] start_escape_processing: content_start={}, content_end={}, copying {} bytes",
                    content_start, content_end, content_end.wrapping_sub(content_start));

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

impl<'b> ContentBuilder for StreamContentBuilder<'b> {
    fn begin_content(&mut self, _pos: usize, _is_key: bool) {
        // StreamParser doesn't need explicit content begin processing
        // as it handles content accumulation automatically
    }

    fn handle_simple_escape(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)
    }

    fn handle_unicode_escape(&mut self, utf8_bytes: &[u8]) -> Result<(), ParseError> {
        // StreamParser handles all escape sequences the same way - append bytes to escape buffer
        for &byte in utf8_bytes {
            self.stream_buffer
                .append_unescaped_byte(byte)
                .map_err(ParseError::from)?;
        }
        Ok(())
    }

    fn append_literal_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        // Check if we're in a string or key state and should accumulate bytes
        let in_string_mode = matches!(self.parser_state, State::String(_) | State::Key(_));

        if in_string_mode {
            // CRITICAL FIX: If we're inside a Unicode escape sequence, route hex digits to the collector
            if self.in_unicode_escape {
                log::debug!(
                    "[NEW] Unicode escape hex digit: {:02x} ('{}')",
                    byte,
                    byte as char
                );

                // Try to add the hex digit to the collector
                let is_complete = self.unicode_escape_collector.add_hex_digit(byte)?;

                if is_complete {
                    log::debug!("[NEW] Unicode escape sequence complete, processing to UTF-8");

                    // Process the complete sequence to UTF-8
                    let mut utf8_buf = [0u8; 4];
                    let (utf8_bytes_opt, _) = self
                        .unicode_escape_collector
                        .process_to_utf8(&mut utf8_buf)?;

                    // Write UTF-8 bytes to escape buffer if we have them
                    if let Some(utf8_bytes) = utf8_bytes_opt {
                        for &utf8_byte in utf8_bytes {
                            self.stream_buffer
                                .append_unescaped_byte(utf8_byte)
                                .map_err(ParseError::from)?;
                        }
                    }

                    // Clear the Unicode escape state - we'll get End(UnicodeEscape) event next
                    self.in_unicode_escape = false;
                }

                return Ok(());
            }

            // Normal literal byte processing (not inside Unicode escape)
            // Skip writing bytes to escape buffer when we have a pending high surrogate
            // (prevents literal \uD801 text from being included in final string)
            if self.stream_buffer.has_unescaped_content()
                && !self.unicode_escape_collector.has_pending_high_surrogate()
            {
                self.stream_buffer
                    .append_unescaped_byte(byte)
                    .map_err(ParseError::from)?;
            }
        }

        Ok(())
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        log::debug!("[NEW] begin_escape_sequence() called");
        self.start_escape_processing()
    }

    fn extract_string(&mut self, start_pos: usize) -> Result<String<'_, '_>, ParseError> {
        if self.stream_buffer.has_unescaped_content() {
            self.create_unescaped_string()
        } else {
            self.create_borrowed_string(start_pos)
        }
    }

    fn extract_key(&mut self, start_pos: usize) -> Result<String<'_, '_>, ParseError> {
        if self.stream_buffer.has_unescaped_content() {
            self.queue_unescaped_reset();
            let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
            let str_content = crate::shared::from_utf8(unescaped_slice)?;
            Ok(String::Unescaped(str_content))
        } else {
            self.create_borrowed_string(start_pos)
        }
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // Use shared number parsing with StreamParser-specific document end detection
        // StreamParser uses state-based detection: finished flag indicates true document end
        let at_document_end = finished;
        log::debug!("[NEW] extract_number: start_pos={}, from_container_end={}, finished={}, at_document_end={}",
            start_pos, from_container_end, finished, at_document_end);
        crate::number_parser::parse_number_with_delimiter_logic(
            &self.stream_buffer,
            start_pos,
            from_container_end,
            at_document_end,
        )
    }

    fn current_position(&self) -> usize {
        self.stream_buffer.current_position()
    }

    fn is_exhausted(&self) -> bool {
        self.stream_buffer.is_empty()
    }
}

impl<'b> ContentExtractor for StreamContentBuilder<'b> {
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
        let string = if self.stream_buffer.has_unescaped_content() {
            self.create_unescaped_string()?
        } else {
            self.create_borrowed_string(start_pos)?
        };
        Ok(Event::String(string))
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        let key = if self.stream_buffer.has_unescaped_content() {
            self.queue_unescaped_reset();
            let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
            let str_content = crate::shared::from_utf8(unescaped_slice)?;
            String::Unescaped(str_content)
        } else {
            self.create_borrowed_string(start_pos)?
        };
        Ok(Event::Key(key))
    }

    fn extract_number_content(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // LEGACY METHOD: This should be updated to use the new ContentBuilder::extract_number
        // For now, use conservative approach - assume not finished for ContentExtractor calls
        let finished = false; // Conservative: assume not at document end for legacy calls
        let at_document_end = finished;
        log::debug!("[NEW] extract_number_content LEGACY: start_pos={}, from_container_end={}, assuming finished={}, at_document_end={}",
            start_pos, from_container_end, finished, at_document_end);
        crate::number_parser::parse_number_with_delimiter_logic(
            &self.stream_buffer,
            start_pos,
            from_container_end,
            at_document_end,
        )
    }
}

impl<'b> EscapeHandler for StreamContentBuilder<'b> {
    fn parser_state(&self) -> &State {
        &self.parser_state
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Shared Unicode escape processing pattern - collect UTF-8 bytes first to avoid borrow conflicts
        let utf8_bytes_result = {
            let current_pos = self.stream_buffer.current_position();
            let hex_slice_provider = |start, end| {
                self.stream_buffer
                    .get_string_slice(start, end)
                    .map_err(Into::into)
            };

            let mut utf8_buf = [0u8; 4];
            let (utf8_bytes_opt, _escape_start_pos) =
                crate::escape_processor::process_unicode_escape_sequence(
                    current_pos,
                    &mut self.unicode_escape_collector,
                    hex_slice_provider,
                    &mut utf8_buf,
                )?;

            // Copy UTF-8 bytes to avoid borrow conflicts
            utf8_bytes_opt.map(|bytes| {
                let mut copy = [0u8; 4];
                let len = bytes.len();
                if let Some(dest) = copy.get_mut(..len) {
                    dest.copy_from_slice(bytes);
                }
                (copy, len)
            })
        };

        // Handle UTF-8 bytes if we have them (not a high surrogate waiting for low surrogate)
        if let Some((utf8_bytes, len)) = utf8_bytes_result {
            // StreamParser handles all escape sequences the same way - append bytes to escape buffer
            // Use safe slice access to avoid panic
            if let Some(valid_bytes) = utf8_bytes.get(..len) {
                for &byte in valid_bytes {
                    self.stream_buffer
                        .append_unescaped_byte(byte)
                        .map_err(ParseError::from)?;
                }
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)
    }

    fn append_literal_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        // Check if we're in a string or key state and should accumulate bytes
        let in_string_mode = matches!(self.parser_state, State::String(_) | State::Key(_));

        if in_string_mode {
            // Skip writing bytes to escape buffer when we have a pending high surrogate
            // (prevents literal \uD801 text from being included in final string)
            if self.stream_buffer.has_unescaped_content()
                && !self.unicode_escape_collector.has_pending_high_surrogate()
            {
                self.stream_buffer
                    .append_unescaped_byte(byte)
                    .map_err(ParseError::from)?;
            }
        }

        Ok(())
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        log::debug!("[NEW] EscapeHandler::begin_escape_sequence() called");
        // Delegate to ContentBuilder implementation
        ContentBuilder::begin_escape_sequence(self)
    }
}
