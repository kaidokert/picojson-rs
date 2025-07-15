// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for StreamParser using StreamBuffer.

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
    /// Flag to track when we're inside ANY escape sequence (like old implementation)
    in_escape_sequence: bool,
    /// Flag to track when the input stream has been finished (for number parsing)
    finished: bool,
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
            in_escape_sequence: false,
            finished: false,
        }
    }

    /// Fill the buffer from a reader
    pub fn fill_buffer_from_reader<R: crate::stream_parser::Reader>(
        &mut self,
        reader: &mut R,
    ) -> Result<(), crate::ParseError> {
        // If buffer is full, try to compact it first (original compaction logic)
        if self.stream_buffer.get_fill_slice().is_none() {
            // Buffer is full - ALWAYS attempt compaction
            let compact_start_pos = match self.parser_state {
                crate::shared::State::Number(start_pos) => start_pos,
                crate::shared::State::Key(start_pos) => start_pos,
                crate::shared::State::String(start_pos) => start_pos,
                _ => self.stream_buffer.current_position(),
            };

            let compaction_offset = self
                .stream_buffer
                .compact_from(compact_start_pos)
                .map_err(crate::ParseError::from)?;

            if compaction_offset == 0 {
                // SOL: Buffer too small for current token
                return Err(crate::ParseError::ScratchBufferFull);
            }

            // Update parser state positions after compaction (original logic)
            self.update_positions_after_compaction(compaction_offset)?;
        }

        if let Some(fill_slice) = self.stream_buffer.get_fill_slice() {
            let bytes_read = reader
                .read(fill_slice)
                .map_err(|_| crate::ParseError::ReaderError)?;
            self.stream_buffer
                .mark_filled(bytes_read)
                .map_err(crate::ParseError::from)?;
        }
        Ok(())
    }

    /// Update parser state positions after compaction (original logic)
    fn update_positions_after_compaction(
        &mut self,
        compaction_offset: usize,
    ) -> Result<(), crate::ParseError> {
        // Update positions - since we compact from the token start position,
        // positions should not be discarded in normal operation
        match &mut self.parser_state {
            crate::shared::State::None => {
                // No position-based state to update
            }
            crate::shared::State::Key(pos) => {
                if *pos >= compaction_offset {
                    *pos = pos.checked_sub(compaction_offset).unwrap_or(0);
                } else {
                    // This shouldn't happen since we compact from the token start
                    *pos = 0;
                }
            }
            crate::shared::State::String(pos) => {
                if *pos >= compaction_offset {
                    *pos = pos.checked_sub(compaction_offset).unwrap_or(0);
                } else {
                    // This shouldn't happen since we compact from the token start
                    *pos = 0;
                }
            }
            crate::shared::State::Number(pos) => {
                if *pos >= compaction_offset {
                    *pos = pos.checked_sub(compaction_offset).unwrap_or(0);
                } else {
                    // This shouldn't happen since we compact from the token start
                    *pos = 0;
                }
            }
        }
        Ok(())
    }

    /// Get access to the stream buffer for byte operations
    pub fn stream_buffer(&self) -> &StreamBuffer<'b> {
        &self.stream_buffer
    }

    /// Get mutable access to the stream buffer for byte operations
    pub fn stream_buffer_mut(&mut self) -> &mut StreamBuffer<'b> {
        &mut self.stream_buffer
    }

    /// Set the finished state (called by StreamParser when input is exhausted)
    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
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

impl ContentExtractor for StreamContentBuilder<'_> {
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
        // Use shared number parsing with StreamParser-specific document end detection
        // StreamParser uses state-based detection: finished flag indicates true document end
        let at_document_end = self.finished;
        crate::number_parser::parse_number_with_delimiter_logic(
            &self.stream_buffer,
            start_pos,
            from_container_end,
            at_document_end,
        )
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
        crate::number_parser::parse_number_with_delimiter_logic(
            &self.stream_buffer,
            start_pos,
            from_container_end,
            at_document_end,
        )
    }
}

impl crate::shared::ByteProvider for StreamContentBuilder<'_> {
    fn next_byte(&mut self) -> Result<Option<u8>, crate::ParseError> {
        // This implementation doesn't have access to the reader
        // It relies on StreamParser to fill the buffer before calling the unified method

        // If buffer is empty, cannot provide bytes
        if self.stream_buffer.is_empty() {
            return Ok(None);
        }

        // Get byte and advance
        let byte = self.stream_buffer.current_byte()?;
        self.stream_buffer.advance()?;

        Ok(Some(byte))
    }
}

/// Custom ByteProvider that can handle reader filling for StreamParser
pub struct StreamContentBuilderWithFiller<'a, 'b, R: crate::stream_parser::Reader> {
    content_builder: &'a mut StreamContentBuilder<'b>,
    reader: &'a mut R,
    finished: &'a mut bool,
}

impl<R: crate::stream_parser::Reader> StreamContentBuilderWithFiller<'_, '_, R> {}

impl<R: crate::stream_parser::Reader> crate::shared::ByteProvider
    for StreamContentBuilderWithFiller<'_, '_, R>
{
    fn next_byte(&mut self) -> Result<Option<u8>, crate::ParseError> {
        // If buffer is empty, try to fill it first
        if self.content_builder.stream_buffer.is_empty() {
            self.content_builder.fill_buffer_from_reader(self.reader)?;
        }

        // If still empty after fill attempt, we're at EOF
        if self.content_builder.stream_buffer.is_empty() {
            // Set finished flag when we reach end of stream
            if !*self.finished {
                *self.finished = true;
                self.content_builder.set_finished(true);
            }
            return Ok(None);
        }

        // Get byte and advance
        let byte = self.content_builder.stream_buffer.current_byte()?;
        self.content_builder.stream_buffer.advance()?;
        Ok(Some(byte))
    }
}

/// Provider that can fill the buffer from a reader
pub struct StreamContentBuilderWithReader<'a, 'b, R: crate::stream_parser::Reader> {
    pub content_builder: &'a mut StreamContentBuilder<'b>,
    reader: &'a mut R,
    finished: &'a mut bool,
}

impl<R: crate::stream_parser::Reader> StreamContentBuilderWithReader<'_, '_, R> {}

impl<R: crate::stream_parser::Reader> crate::shared::ByteProvider
    for StreamContentBuilderWithReader<'_, '_, R>
{
    fn next_byte(&mut self) -> Result<Option<u8>, crate::ParseError> {
        // If buffer is empty, try to fill it first
        if self.content_builder.stream_buffer.is_empty() {
            if let Some(fill_slice) = self.content_builder.stream_buffer.get_fill_slice() {
                let bytes_read = self
                    .reader
                    .read(fill_slice)
                    .map_err(|_| crate::ParseError::ReaderError)?;
                self.content_builder
                    .stream_buffer
                    .mark_filled(bytes_read)
                    .map_err(crate::ParseError::from)?;
            }
        }

        // If still empty after fill attempt, we're at EOF
        if self.content_builder.stream_buffer.is_empty() {
            // Set finished flag when we reach end of stream
            if !*self.finished {
                *self.finished = true;
                self.content_builder.set_finished(true);
            }
            return Ok(None);
        }

        // Get byte and advance
        let byte = self.content_builder.stream_buffer.current_byte()?;
        self.content_builder.stream_buffer.advance()?;
        Ok(Some(byte))
    }
}

impl<R: crate::stream_parser::Reader> crate::event_processor::ContentExtractor
    for StreamContentBuilderWithReader<'_, '_, R>
{
    fn parser_state_mut(&mut self) -> &mut crate::shared::State {
        self.content_builder.parser_state_mut()
    }

    fn current_position(&self) -> usize {
        self.content_builder.current_position()
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.content_builder.begin_string_content(pos);
    }

    fn unicode_escape_collector_mut(
        &mut self,
    ) -> &mut crate::escape_processor::UnicodeEscapeCollector {
        self.content_builder.unicode_escape_collector_mut()
    }

    fn extract_string_content(
        &mut self,
        start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        self.content_builder.extract_string_content(start_pos)
    }

    fn extract_key_content(
        &mut self,
        start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        self.content_builder.extract_key_content(start_pos)
    }

    fn extract_number_content(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        self.content_builder
            .extract_number_content(start_pos, from_container_end)
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        self.content_builder
            .extract_number(start_pos, from_container_end, finished)
    }
}

impl<R: crate::stream_parser::Reader> crate::event_processor::EscapeHandler
    for StreamContentBuilderWithReader<'_, '_, R>
{
    fn parser_state(&self) -> &crate::shared::State {
        self.content_builder.parser_state()
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), crate::ParseError> {
        self.content_builder.process_unicode_escape_with_collector()
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), crate::ParseError> {
        self.content_builder.handle_simple_escape_char(escape_char)
    }

    fn begin_escape_sequence(&mut self) -> Result<(), crate::ParseError> {
        self.content_builder.begin_escape_sequence()
    }

    fn begin_unicode_escape(&mut self) -> Result<(), crate::ParseError> {
        self.content_builder.begin_unicode_escape()
    }
}

impl StreamContentBuilder<'_> {
    /// Handle byte accumulation for StreamParser-specific requirements
    /// This method is called when a byte doesn't generate any events
    pub fn handle_byte_accumulation(&mut self, byte: u8) -> Result<(), crate::ParseError> {
        // Check if we're in a string or key state and should accumulate bytes
        let in_string_mode = matches!(self.parser_state, State::String(_) | State::Key(_));

        if in_string_mode {
            // When unescaped content is active, we need to accumulate ALL string content
            // This includes both regular characters and content after escape sequences
            if self.stream_buffer.has_unescaped_content() {
                // Follow old implementation pattern - do NOT write to escape buffer
                // when inside ANY escape sequence (in_escape_sequence == true)
                // This prevents hex digits from being accumulated as literal text
                if !self.in_escape_sequence
                    && !self.unicode_escape_collector.has_pending_high_surrogate()
                {
                    self.stream_buffer
                        .append_unescaped_byte(byte)
                        .map_err(crate::ParseError::from)?;
                }
            }
        }
        Ok(())
    }
}

impl EscapeHandler for StreamContentBuilder<'_> {
    fn parser_state(&self) -> &State {
        &self.parser_state
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        // Called when Begin(UnicodeEscape) is received
        self.in_unicode_escape = true;
        self.in_escape_sequence = true;
        Ok(())
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Reset the escape flags
        self.in_unicode_escape = false;
        self.in_escape_sequence = false;
        // Shared Unicode escape processing pattern - collect UTF-8 bytes first to avoid borrow conflicts
        let utf8_bytes_result = {
            let current_pos = self.stream_buffer.current_position();
            let hex_slice_provider = |start, end| {
                self.stream_buffer
                    .get_string_slice(start, end)
                    .map_err(Into::into)
            };

            let mut utf8_buf = [0u8; 4];
            let (utf8_bytes_opt, _) = crate::escape_processor::process_unicode_escape_sequence(
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
        // Clear the escape sequence flag when simple escape completes
        self.in_escape_sequence = false;
        self.stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        self.in_escape_sequence = true;
        self.start_escape_processing()
    }
}
