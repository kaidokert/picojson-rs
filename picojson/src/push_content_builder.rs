// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for PushParser using SAX-style event handling.

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

/// ContentBuilder implementation for PushParser that handles SAX-style event emission
pub struct PushContentBuilder<'scratch, H> {
    /// The handler that receives events
    handler: H,
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
}

impl<'scratch, H> PushContentBuilder<'scratch, H> {
    /// Create a new PushContentBuilder
    pub fn new(handler: H, buffer: &'scratch mut [u8]) -> Self {
        Self {
            handler,
            stream_buffer: StreamBuffer::new(buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            unescaped_reset_queued: false,
            position_offset: 0,
            current_position: 0,
            token_start_pos: 0,
            using_unescaped_buffer: false,
        }
    }

    /// Update the current position
    pub fn set_current_position(&mut self, pos: usize) {
        self.current_position = pos;
    }

    /// Update the position offset for chunk processing
    pub fn set_position_offset(&mut self, offset: usize) {
        self.position_offset = offset;
    }

    /// Update position offset by adding to it
    pub fn add_position_offset(&mut self, amount: usize) {
        self.position_offset += amount;
    }

    /// Set the token start position
    pub fn set_token_start_pos(&mut self, pos: usize) {
        self.token_start_pos = pos;
    }

    /// Get the token start position
    pub fn token_start_pos(&self) -> usize {
        self.token_start_pos
    }

    /// Set whether we're using the unescaped buffer
    pub fn set_using_unescaped_buffer(&mut self, using: bool) {
        self.using_unescaped_buffer = using;
    }

    /// Check if we're using the unescaped buffer
    pub fn using_unescaped_buffer(&self) -> bool {
        self.using_unescaped_buffer
    }

    /// Clear the unescaped buffer
    pub fn clear_unescaped(&mut self) {
        self.stream_buffer.clear_unescaped();
    }

    /// Append a byte to the unescaped buffer
    pub fn append_unescaped_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(byte)
            .map_err(ParseError::from)
    }

    /// Get access to the stream buffer
    pub fn stream_buffer(&self) -> &StreamBuffer<'scratch> {
        &self.stream_buffer
    }

    /// Get the position offset
    pub fn position_offset(&self) -> usize {
        self.position_offset
    }

    /// Emit an event through the handler
    pub fn emit_event<E>(&mut self, event: Event<'_, '_>) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        self.handler.handle_event(event)
    }

    /// Get mutable access to the unicode escape collector
    pub fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
        &mut self.unicode_escape_collector
    }

    /// Get ownership of the handler (for destroy method)
    pub fn into_handler(self) -> H {
        self.handler
    }

    /// Finishes parsing and returns the handler.
    pub fn finish<E>(mut self) -> Result<H, crate::push_parser::PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Handle any remaining content in the buffer
        if self.parser_state != State::None {
            return Err(crate::push_parser::PushParseError::Parse(
                ParseError::EndOfData,
            ));
        }

        // Emit EndDocument event
        self.handler
            .handle_event(Event::EndDocument)
            .map_err(crate::push_parser::PushParseError::Handler)?;

        Ok(self.handler)
    }

    /// Emit a number event from the unescaped buffer and clear it atomically
    pub fn emit_number_from_unescaped_buffer<E>(
        &mut self,
    ) -> Result<(), crate::push_parser::PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
        let num = JsonNumber::from_slice(unescaped_slice)?;
        let event = Event::Number(num);

        // Emit the event first while we still have the borrow
        let result = self
            .handler
            .handle_event(event)
            .map_err(crate::push_parser::PushParseError::Handler)?;

        // Clear buffer after event is handled
        self.stream_buffer.clear_unescaped();

        Ok(result)
    }

    /// Extracts string or key content, emits the corresponding event, and queues a buffer reset.
    /// This method delegates the core extraction logic to `get_content_piece` to avoid
    /// logic duplication, while providing a clean API for the `PushParser`.
    pub fn extract_string_or_key_content<'input, E>(
        &mut self,
        is_key: bool,
        data: &'input [u8],
    ) -> Result<(), crate::push_parser::PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Create a DataSource on the fly to pass to the shared content extraction logic.
        let data_source = PushParserDataSource {
            input_data: data,
            stream_buffer: &self.stream_buffer,
            using_unescaped_buffer: self.using_unescaped_buffer,
            position_offset: self.position_offset,
        };

        // The start position for the content is after the opening quote.
        let content_start_pos = self.token_start_pos + 1;
        // The end position for get_content_piece is one *after* the closing quote.
        let content_end_pos = self.current_position + 1;

        // Delegate the core if/else logic to the shared function.
        let content_piece =
            crate::shared::get_content_piece(&data_source, content_start_pos, content_end_pos)?;
        let content_string = content_piece.to_string()?;

        let event = if is_key {
            Event::Key(content_string)
        } else {
            Event::String(content_string)
        };

        // Emit the event and queue the buffer to be reset.
        self.handler
            .handle_event(event)
            .map_err(crate::push_parser::PushParseError::Handler)?;
        self.queue_unescaped_reset();
        Ok(())
    }

    /// Emit a number event using DataSource pattern and clear buffer atomically
    pub fn emit_number_event<'input, E>(
        &mut self,
        data: &'input [u8],
        start_pos: usize,
        end_pos: usize,
    ) -> Result<(), crate::push_parser::PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Create DataSource for content extraction
        let data_source = PushParserDataSource {
            input_data: data,
            stream_buffer: &self.stream_buffer,
            using_unescaped_buffer: self.using_unescaped_buffer,
            position_offset: self.position_offset,
        };

        let number_bytes = {
            let content_piece = crate::shared::get_content_piece(&data_source, start_pos, end_pos)?;
            content_piece.as_bytes()
        };

        let num = JsonNumber::from_slice(number_bytes)?;
        let event = Event::Number(num);

        // Emit the event first while we still have the borrow
        let result = self
            .handler
            .handle_event(event)
            .map_err(crate::push_parser::PushParseError::Handler)?;

        // Clear buffer after event is handled
        self.stream_buffer.clear_unescaped();

        Ok(result)
    }

    /// Apply queued unescaped content reset if needed
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
}

impl<H> ContentExtractor for PushContentBuilder<'_, H> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        // PushParser feeds bytes externally, so this is not used
        // Return None to indicate no more bytes available
        Ok(None)
    }

    fn current_position(&self) -> usize {
        self.current_position
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.token_start_pos = pos;
        self.using_unescaped_buffer = false;
        self.stream_buffer.clear_unescaped();
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

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        let string = if self.using_unescaped_buffer {
            let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
            let str_content = crate::shared::from_utf8(unescaped_slice)?;
            crate::String::Unescaped(str_content)
        } else {
            let (content_start, content_end) =
                crate::shared::ContentRange::string_content_bounds_from_content_start(
                    start_pos,
                    self.current_position,
                );
            let bytes = self
                .stream_buffer
                .get_string_slice(content_start, content_end)?;
            let str_content = crate::shared::from_utf8(bytes)?;
            crate::String::Borrowed(str_content)
        };
        Ok(Event::String(string))
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        let key = if self.using_unescaped_buffer {
            let unescaped_slice = self.stream_buffer.get_unescaped_slice()?;
            let str_content = crate::shared::from_utf8(unescaped_slice)?;
            crate::String::Unescaped(str_content)
        } else {
            let (content_start, content_end) =
                crate::shared::ContentRange::string_content_bounds_from_content_start(
                    start_pos,
                    self.current_position,
                );
            let bytes = self
                .stream_buffer
                .get_string_slice(content_start, content_end)?;
            let str_content = crate::shared::from_utf8(bytes)?;
            crate::String::Borrowed(str_content)
        };
        Ok(Event::Key(key))
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        let current_pos = self.current_position;
        let use_full_span = !from_container_end && finished;
        let end_pos = crate::shared::ContentRange::number_end_position(current_pos, use_full_span);

        let number_bytes = if self.using_unescaped_buffer {
            self.stream_buffer.get_unescaped_slice()?
        } else {
            self.stream_buffer.get_string_slice(start_pos, end_pos)?
        };

        let json_number = JsonNumber::from_slice(number_bytes)?;
        Ok(Event::Number(json_number))
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        let mut utf8_buffer = [0u8; 4];
        match self
            .unicode_escape_collector
            .process_to_utf8(&mut utf8_buffer)
        {
            Ok((utf8_bytes, _)) => {
                if let Some(bytes) = utf8_bytes {
                    for &b in bytes {
                        self.stream_buffer.append_unescaped_byte(b)?;
                    }
                }
            }
            Err(e) => return Err(e.into()),
        }

        self.unicode_escape_collector.reset();
        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        // Mark that we're in an escape sequence
        // Switch to unescaped mode will be handled when we get the escape content
        Ok(())
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        // Start of unicode escape sequence - reset collector for new sequence
        self.unicode_escape_collector.reset();
        Ok(())
    }
}

/// Helper struct that implements DataSource for PushParser content extraction
pub struct PushParserDataSource<'input, 'scratch> {
    /// Current input data chunk being processed
    pub input_data: &'input [u8],
    /// Reference to the stream buffer for unescaped content
    pub stream_buffer: &'scratch StreamBuffer<'scratch>,
    /// Whether unescaped content is being used
    pub using_unescaped_buffer: bool,
    /// Position offset for converting absolute positions to slice positions
    pub position_offset: usize,
}

impl<'input, 'scratch> DataSource<'input, 'scratch> for PushParserDataSource<'input, 'scratch> {
    fn get_borrowed_slice(
        &'input self,
        start: usize,
        end: usize,
    ) -> Result<&'input [u8], ParseError> {
        // Convert absolute positions to relative positions within the current data chunk
        let slice_start = start.saturating_sub(self.position_offset);
        let slice_end = end.saturating_sub(self.position_offset);

        if slice_end > self.input_data.len() || slice_start > slice_end {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }

        Ok(&self.input_data[slice_start..slice_end])
    }

    fn get_unescaped_slice(&'scratch self) -> Result<&'scratch [u8], ParseError> {
        self.stream_buffer
            .get_unescaped_slice()
            .map_err(ParseError::from)
    }

    fn has_unescaped_content(&self) -> bool {
        self.using_unescaped_buffer && self.stream_buffer.has_unescaped_content()
    }
}
