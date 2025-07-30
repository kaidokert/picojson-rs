// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for SliceParser using CopyOnEscape optimization.

use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::{self, UnicodeEscapeCollector};
use crate::event_processor::{ContentExtractor, DataSource};
use crate::shared::{ContentRange, State};
use crate::slice_input_buffer::{InputBuffer, SliceInputBuffer};
use crate::{Event, JsonNumber, ParseError};

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
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        match self.buffer_mut().consume_byte() {
            Ok(byte) => Ok(Some(byte)),
            Err(crate::slice_input_buffer::Error::ReachedEnd) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn parser_state_mut(&mut self) -> &mut State {
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

    fn extract_string_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let value_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(Event::String(value_result))
    }

    fn extract_key_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let key_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(Event::Key(key_result))
    }

    fn extract_key_content_new<'input, 'scratch>(
        &mut self,
        source: &impl DataSource<'input, 'scratch>,
        start_pos: usize,
    ) -> Result<Event<'input, 'scratch>, ParseError> {
        // For SliceParser, we can only use borrowed content due to lifetime constraints
        // In a full implementation, this would be resolved by architectural changes
        if source.has_unescaped_content() {
            // This demonstrates the intent but currently returns an error due to lifetime issues
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ));
        }

        // Get borrowed content from input (this works)
        let current_pos = self.buffer.current_pos();
        let (content_start, content_end) =
            ContentRange::string_content_bounds_from_content_start(start_pos, current_pos);

        let content_bytes = source.get_borrowed_slice(content_start, content_end)?;
        let content_str = core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
        Ok(Event::Key(crate::String::Borrowed(content_str)))
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        _finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
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
        let json_number = JsonNumber::from_slice(number_bytes)?;
        Ok(Event::Number(json_number))
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        Ok(())
    }

    fn parser_state(&self) -> &State {
        &self.parser_state
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        let current_pos = self.buffer.current_pos();
        let hex_slice_provider = |start, end| self.buffer.slice(start, end).map_err(Into::into);

        // Shared Unicode escape processing pattern
        let had_pending_high_surrogate = self.unicode_escape_collector.has_pending_high_surrogate();

        let (utf8_bytes_result, escape_start_pos) =
            escape_processor::process_unicode_escape_sequence(
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

    /// Override the standard validation method to use DataSource internally
    fn validate_and_extract_key(&mut self) -> Result<Event<'_, '_>, ParseError> {
        let start_pos = match *self.parser_state() {
            State::Key(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the key
        if self
            .unicode_escape_collector_mut()
            .has_pending_high_surrogate()
        {
            return Err(ParseError::InvalidUnicodeCodepoint);
        }

        *self.parser_state_mut() = State::None;

        // Use DataSource pattern manually to avoid borrowing conflicts
        if self.copy_on_escape.has_unescaped_content() {
            // Get unescaped content from scratch buffer
            let content_bytes = self.copy_on_escape.get_unescaped_slice()?;
            let content_str =
                core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
            Ok(Event::Key(crate::String::Unescaped(content_str)))
        } else {
            // Get borrowed content from input
            let current_pos = self.buffer.current_pos();
            let (content_start, content_end) =
                ContentRange::string_content_bounds_from_content_start(start_pos, current_pos);

            let content_bytes = self.buffer.slice(content_start, content_end).map_err(|_| {
                ParseError::Unexpected(crate::shared::UnexpectedState::InvalidSliceBounds)
            })?;
            let content_str =
                core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
            Ok(Event::Key(crate::String::Borrowed(content_str)))
        }
    }

    /// Override the new DataSource-based validation method for SliceParser
    fn validate_and_extract_key_with_source<'input, 'scratch>(
        &mut self,
        source: &impl crate::event_processor::DataSource<'input, 'scratch>,
    ) -> Result<crate::Event<'input, 'scratch>, ParseError> {
        let start_pos = match *self.parser_state() {
            State::Key(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the key
        if self
            .unicode_escape_collector_mut()
            .has_pending_high_surrogate()
        {
            return Err(ParseError::InvalidUnicodeCodepoint);
        }

        *self.parser_state_mut() = State::None;

        // Use the new DataSource-based extraction
        self.extract_key_content_new(source, start_pos)
    }
}

// DataSource implementation for SliceContentBuilder demonstrates the pattern
// but has fundamental lifetime challenges due to the current architecture
impl<'a, 'b> DataSource<'a, 'a> for SliceContentBuilder<'a, 'b> {
    fn get_borrowed_slice(&self, start: usize, end: usize) -> Result<&'a [u8], ParseError> {
        // SliceParser can directly slice from the input using SliceInputBuffer
        self.buffer
            .slice(start, end)
            .map_err(|_| ParseError::Unexpected(crate::shared::UnexpectedState::InvalidSliceBounds))
    }

    fn get_unescaped_slice(&self) -> Result<&'a [u8], ParseError> {
        // This is a fundamental limitation: CopyOnEscape returns &'b references
        // but DataSource expects &'a references. In a full implementation,
        // this would require architectural changes to align the lifetimes.
        Err(ParseError::Unexpected(
            crate::shared::UnexpectedState::StateMismatch,
        ))
    }

    fn has_unescaped_content(&self) -> bool {
        // This method works fine as it doesn't involve lifetimes
        self.copy_on_escape.has_unescaped_content()
    }
}
