// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for SliceParser using CopyOnEscape optimization.

use crate::content_builder::ContentBuilder;
use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::{ContentExtractor, EscapeHandler};
use crate::number_parser::NumberExtractor;
use crate::shared::{ContentRange, State};
use crate::slice_input_buffer::SliceInputBuffer;
use crate::{Event, ParseError, String};

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
    /// Flag to track when we're inside ANY escape sequence (like stream implementation)
    in_escape_sequence: bool,
}

impl<'a, 'b> SliceContentBuilder<'a, 'b> {
    /// Create a new SliceContentBuilder
    pub fn new(input: &'a [u8], scratch_buffer: &'b mut [u8]) -> Self {
        Self {
            buffer: SliceInputBuffer::new(input),
            copy_on_escape: CopyOnEscape::new(input, scratch_buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            in_escape_sequence: false,
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

impl ContentBuilder for SliceContentBuilder<'_, '_> {
    fn begin_content(&mut self, pos: usize, _is_key: bool) {
        // SliceParser uses CopyOnEscape for content management
        self.copy_on_escape.begin_string(pos);
    }

    fn handle_simple_escape(&mut self, escape_char: u8) -> Result<(), ParseError> {
        // Clear the escape sequence flag when simple escape completes
        self.in_escape_sequence = false;
        self.copy_on_escape
            .handle_escape(self.buffer.current_pos(), escape_char)?;
        Ok(())
    }

    fn handle_unicode_escape(&mut self, utf8_bytes: &[u8]) -> Result<(), ParseError> {
        // SliceParser handles Unicode escapes through CopyOnEscape
        // The position calculation matches the existing SliceParser logic
        let current_pos = self.buffer.current_pos();
        let (_, _, escape_start_pos) = ContentRange::unicode_escape_bounds(current_pos);
        // Fix: escape_start_pos should point to the backslash, not the 'u'
        let actual_escape_start_pos = escape_start_pos.saturating_sub(1);

        // Check if this is completing a surrogate pair
        let had_pending_high_surrogate = self.unicode_escape_collector.has_pending_high_surrogate();

        if had_pending_high_surrogate {
            // This is completing a surrogate pair - need to consume both escapes
            // First call: consume the high surrogate (6 bytes earlier)
            self.copy_on_escape
                .handle_unicode_escape(actual_escape_start_pos, &[])?;
            // Second call: consume the low surrogate and write UTF-8
            self.copy_on_escape
                .handle_unicode_escape(actual_escape_start_pos + 6, utf8_bytes)?;
        } else {
            // Single Unicode escape - normal processing
            self.copy_on_escape
                .handle_unicode_escape(actual_escape_start_pos, utf8_bytes)?;
        }

        Ok(())
    }

    fn append_literal_byte(&mut self, _byte: u8) -> Result<(), ParseError> {
        // SliceParser doesn't typically need per-byte processing since it works with ranges
        // This could be implemented as a single-byte range if needed, but for now it's a no-op
        Ok(())
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        // Set escape flag to prevent literal byte accumulation during escape processing
        self.in_escape_sequence = true;
        Ok(())
    }

    fn extract_string(&mut self, _start_pos: usize) -> Result<String<'_, '_>, ParseError> {
        // Use CopyOnEscape to get the final string result
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let value_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(value_result)
    }

    fn extract_key(&mut self, _start_pos: usize) -> Result<String<'_, '_>, ParseError> {
        // Use CopyOnEscape to get the final key result
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let key_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(key_result)
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // Use shared number parsing with SliceParser-specific document end detection
        // SliceParser uses buffer-based detection: buffer empty indicates document end
        let at_document_end = self.buffer.is_empty();
        log::debug!("[NEW] SliceContentBuilder extract_number: start_pos={}, from_container_end={}, finished={} (ignored), buffer_empty={}, at_document_end={}",
            start_pos, from_container_end, finished, self.buffer.is_empty(), at_document_end);
        // SliceParser-specific fix: at document end, current_pos points past the input,
        // so we need to adjust the logic to always exclude the delimiter position
        let current_pos = self.buffer.current_position();
        let end_pos = current_pos.saturating_sub(1);
        crate::number_parser::parse_number_event(&self.buffer, start_pos, end_pos)
    }

    fn current_position(&self) -> usize {
        self.buffer.current_pos()
    }

    fn is_exhausted(&self) -> bool {
        self.buffer.is_empty()
    }
}

impl ContentExtractor for SliceContentBuilder<'_, '_> {
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

    fn extract_number_content(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
    ) -> Result<crate::Event<'_, '_>, ParseError> {
        // Use shared number parsing with SliceParser-specific document end detection
        // SliceParser uses buffer-based detection: buffer empty indicates document end
        let at_document_end = self.buffer.is_empty();
        log::debug!("[NEW] SliceContentBuilder extract_number_content: start_pos={}, from_container_end={}, buffer_empty={}, at_document_end={}",
            start_pos, from_container_end, self.buffer.is_empty(), at_document_end);
        // SliceParser-specific fix: at document end, current_pos points past the input,
        // so we need to adjust the logic to always exclude the delimiter position
        let current_pos = self.buffer.current_position();
        let end_pos = current_pos.saturating_sub(1);
        crate::number_parser::parse_number_event(&self.buffer, start_pos, end_pos)
    }
}

impl crate::shared::ByteProvider for SliceContentBuilder<'_, '_> {
    fn next_byte(&mut self) -> Result<Option<u8>, crate::ParseError> {
        use crate::slice_input_buffer::InputBuffer;
        match self.buffer_mut().consume_byte() {
            Ok(byte) => Ok(Some(byte)),
            Err(crate::slice_input_buffer::Error::ReachedEnd) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

impl EscapeHandler for SliceContentBuilder<'_, '_> {
    fn parser_state(&self) -> &crate::shared::State {
        &self.parser_state
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Clear the escape sequence flag when unicode escape completes
        self.in_escape_sequence = false;
        let current_pos = self.buffer.current_pos();
        let hex_slice_provider = |start, end| self.buffer.slice(start, end).map_err(Into::into);

        // Shared Unicode escape processing pattern
        let had_pending_high_surrogate = self.unicode_escape_collector.has_pending_high_surrogate();

        let mut utf8_buf = [0u8; 4];
        let (utf8_bytes_opt, escape_start_pos) =
            crate::escape_processor::process_unicode_escape_sequence(
                current_pos,
                &mut self.unicode_escape_collector,
                hex_slice_provider,
                &mut utf8_buf,
            )?;
        // Fix: escape_start_pos should point to the backslash, not the 'u'
        // But don't subtract if it's already pointing to the backslash
        let actual_escape_start_pos = escape_start_pos;

        // Handle UTF-8 bytes if we have them (not a high surrogate waiting for low surrogate)
        if let Some(utf8_bytes) = utf8_bytes_opt {
            if had_pending_high_surrogate {
                // This is completing a surrogate pair - need to consume both escapes
                // First call: consume the high surrogate (6 bytes earlier)
                self.copy_on_escape
                    .handle_unicode_escape(actual_escape_start_pos, &[])?;
                // Second call: consume the low surrogate and write UTF-8
                self.copy_on_escape
                    .handle_unicode_escape(actual_escape_start_pos + 6, utf8_bytes)?;
            } else {
                // Single Unicode escape - normal processing
                self.copy_on_escape
                    .handle_unicode_escape(actual_escape_start_pos, utf8_bytes)?;
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        // Clear the escape sequence flag when simple escape completes
        self.in_escape_sequence = false;
        self.copy_on_escape
            .handle_escape(self.buffer.current_pos(), escape_char)?;
        Ok(())
    }

    fn append_literal_byte(&mut self, _byte: u8) -> Result<(), ParseError> {
        // SliceParser doesn't typically need per-byte processing since it works with ranges
        // This could be implemented as a single-byte range if needed, but for now it's a no-op
        Ok(())
    }
}
