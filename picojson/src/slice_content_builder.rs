// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for SliceParser using CopyOnEscape optimization.

use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::{self, UnicodeEscapeCollector};
use crate::event_processor::ContentExtractor;
use crate::shared::{ContentRange, DataSource, State};
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

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // SliceParser-specific: Complete CopyOnEscape processing for unescaped content
        let current_pos = self.current_position();
        if self.has_unescaped_content() {
            let end_pos = ContentRange::end_position_excluding_delimiter(current_pos);
            self.copy_on_escape.end_string(end_pos)?; // Complete the CopyOnEscape processing
        }

        // Use the unified helper function to get the content
        let content_piece = crate::shared::get_content_piece(self, start_pos, current_pos)?;
        Ok(Event::String(content_piece.into_string()?))
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // SliceParser-specific: Complete CopyOnEscape processing for unescaped content
        let current_pos = self.current_position();
        if self.has_unescaped_content() {
            let end_pos = ContentRange::end_position_excluding_delimiter(current_pos);
            self.copy_on_escape.end_string(end_pos)?; // Complete the CopyOnEscape processing
        }

        // Use the unified helper function to get the content
        let content_piece = crate::shared::get_content_piece(self, start_pos, current_pos)?;
        Ok(Event::Key(content_piece.into_string()?))
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        _from_container_end: bool,
        _finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // The delimiter has already been consumed by the time this is called,
        // so current_position is one byte past the end of the number.
        let end_pos = ContentRange::end_position_excluding_delimiter(self.current_position());

        // Use the DataSource trait method to get the number bytes
        let number_bytes = self.get_borrowed_slice(start_pos, end_pos)?;

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

        // Shared Unicode escape processing pattern
        let had_pending_high_surrogate = self.unicode_escape_collector.has_pending_high_surrogate();
        let pending_surrogate = self.unicode_escape_collector.get_pending_high_surrogate();

        let (utf8_bytes_result, escape_start_pos, new_pending_surrogate) =
            escape_processor::process_unicode_escape_sequence(
                current_pos,
                pending_surrogate,
                self, // Pass self as the DataSource
            )?;

        self.unicode_escape_collector
            .set_pending_high_surrogate(new_pending_surrogate);

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

/// DataSource implementation for SliceContentBuilder
///
/// This implementation provides access to both borrowed content from the original
/// input slice and unescaped content from the CopyOnEscape scratch buffer.
impl<'a, 'b> DataSource<'a, 'b> for SliceContentBuilder<'a, 'b> {
    fn get_borrowed_slice(&'a self, start: usize, end: usize) -> Result<&'a [u8], ParseError> {
        self.buffer.slice(start, end).map_err(Into::into)
    }

    fn get_unescaped_slice(&'b self) -> Result<&'b [u8], ParseError> {
        // Access the scratch buffer directly with the correct lifetime
        if !self.copy_on_escape.has_unescaped_content() {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ));
        }

        // Use the new method with proper lifetime annotation
        let (start, end) = self.copy_on_escape.get_scratch_range();
        self.copy_on_escape.get_scratch_buffer_slice(start, end)
    }

    fn has_unescaped_content(&self) -> bool {
        self.copy_on_escape.has_unescaped_content()
    }
}
