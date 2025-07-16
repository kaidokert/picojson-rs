// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for SliceParser using CopyOnEscape optimization.

use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::ContentExtractor;
use crate::shared::{ContentRange, State};
use crate::slice_input_buffer::{InputBuffer, SliceInputBuffer};
use crate::ParseError;

/// ContentBuilder implementation for SliceParser that uses CopyOnEscape for zero-copy optimization
pub struct SliceContentBuilder<'a, 'b> {
    /// The input buffer for slice-based parsing
    buffer: SliceInputBuffer<'a>,
    /// Copy-on-escape handler for zero-copy string optimization
    copy_on_escape: CopyOnEscape<'a, 'b>,
    /// Parser state tracking
    parser_state: State,
    /// Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
}

impl<'a, 'b> SliceContentBuilder<'a, 'b> {
    /// Create a new SliceContentBuilder
    pub fn new(input: &'a [u8], scratch_buffer: &'b mut [u8]) -> Self {
        Self {
            buffer: SliceInputBuffer::new(input),
            copy_on_escape: CopyOnEscape::new(input, scratch_buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
        }
    }

    /// Get access to the input buffer for byte operations
    pub fn buffer(&self) -> &SliceInputBuffer<'a> {
        &self.buffer
    }

    /// Get mutable access to the input buffer for byte operations
    pub fn buffer_mut(&mut self) -> &mut SliceInputBuffer<'a> {
        &mut self.buffer
    }
}

impl ContentExtractor for SliceContentBuilder<'_, '_> {
    fn next_byte(&mut self) -> Result<Option<u8>, crate::ParseError> {
        match self.buffer_mut().consume_byte() {
            Ok(byte) => Ok(Some(byte)),
            Err(crate::slice_input_buffer::Error::ReachedEnd) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn parser_state_mut(&mut self) -> &mut crate::shared::State {
        &mut self.parser_state
    }

    fn current_position(&self) -> usize {
        self.buffer.current_pos()
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.copy_on_escape.begin_string(pos);
    }

    fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
        &mut self.unicode_escape_collector
    }

    fn extract_string_content(
        &mut self,
        _start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, ParseError> {
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let value_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(crate::Event::String(value_result))
    }

    fn extract_key_content(
        &mut self,
        _start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, ParseError> {
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let key_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(crate::Event::Key(key_result))
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        _finished: bool,
    ) -> Result<crate::Event<'_, '_>, ParseError> {
        // For SliceParser, use buffer-based document end detection
        // The finished parameter should always be true for complete slices, but we don't rely on it
        let at_document_end = self.buffer.current_pos() >= self.buffer.data_len();
        let current_pos = self.buffer.current_pos();
        let use_full_span = !from_container_end && at_document_end;

        let end_pos = if use_full_span {
            // Standalone number: clamp to buffer length to prevent slice bounds errors
            core::cmp::min(current_pos, self.buffer.data_len())
        } else {
            // Container number: exclude delimiter
            current_pos.saturating_sub(1)
        };

        let number_bytes = self
            .buffer
            .slice(start_pos, end_pos)
            .map_err(|_| ParseError::InvalidNumber)?;
        let json_number = crate::JsonNumber::from_slice(number_bytes)?;
        Ok(crate::Event::Number(json_number))
    }

    fn begin_unicode_escape(&mut self) -> Result<(), crate::ParseError> {
        Ok(())
    }

    fn parser_state(&self) -> &crate::shared::State {
        &self.parser_state
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        let current_pos = self.buffer.current_pos();
        let hex_slice_provider = |start, end| self.buffer.slice(start, end).map_err(Into::into);

        // Shared Unicode escape processing pattern
        let had_pending_high_surrogate = self.unicode_escape_collector.has_pending_high_surrogate();

        let (utf8_bytes_result, escape_start_pos) =
            crate::escape_processor::process_unicode_escape_sequence(
                current_pos,
                &mut self.unicode_escape_collector,
                hex_slice_provider,
            )?;

        // Handle UTF-8 bytes if we have them (not a high surrogate waiting for low surrogate)
        if let Some((utf8_bytes, len)) = utf8_bytes_result {
            let utf8_slice = &utf8_bytes[..len];
            if had_pending_high_surrogate {
                // This is completing a surrogate pair - need to consume both escapes
                // First call: consume the high surrogate (6 bytes earlier)
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos, &[])?;
                // Second call: consume the low surrogate and write UTF-8
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos + 6, utf8_slice)?;
            } else {
                // Single Unicode escape - normal processing
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos, utf8_slice)?;
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        // Clear the escape sequence flag when simple escape completes
        self.copy_on_escape
            .handle_escape(self.buffer.current_pos(), escape_char)?;
        Ok(())
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        Ok(())
    }
}
