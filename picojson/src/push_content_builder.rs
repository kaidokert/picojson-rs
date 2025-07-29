// SPDX-License-Identifier: Apache-2.0

//! ContentBuilder implementation for PushParser using SAX-style event handling.

use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::{ContentExtractor, DataSource};
use crate::shared::State;
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

    /// Get the handler back (for destroying the builder)
    pub fn destroy(self) -> H {
        self.handler
    }

    /// Set position tracking information
    pub fn set_position_info(&mut self, position_offset: usize, current_position: usize) {
        self.position_offset = position_offset;
        self.current_position = current_position;
    }

    /// Start tracking a new content token
    pub fn start_content_token(&mut self, pos: usize) {
        self.token_start_pos = pos;
        self.using_unescaped_buffer = false;
        self.stream_buffer.clear_unescaped();
    }

    /// Check if currently using unescaped buffer
    pub fn is_using_unescaped_buffer(&self) -> bool {
        self.using_unescaped_buffer
    }

    /// Mark as using unescaped buffer without copying content
    pub fn mark_using_unescaped_buffer(&mut self) {
        self.using_unescaped_buffer = true;
    }

    /// Get the token start position
    pub fn get_token_start_pos(&self) -> usize {
        self.token_start_pos
    }

    /// Append a byte to the unescaped buffer
    pub fn append_unescaped_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        self.stream_buffer
            .append_unescaped_byte(byte)
            .map_err(ParseError::from)
    }

    /// Switch to unescaped mode and copy content
    pub fn switch_to_unescaped_mode<E>(
        &mut self,
        data: &[u8],
        current_local_pos: usize,
        state: State,
    ) -> Result<(), ParseError> {
        if !self.using_unescaped_buffer {
            // For strings/keys: skip opening quote (+1)
            // For numbers: start from first digit (+0)
            let start_offset = match state {
                State::String(_) | State::Key(_) => 1, // Skip opening quote
                State::Number(_) => 0,                 // Include first digit
                State::None => 0,
            };
            let start_pos = self.token_start_pos + start_offset;
            let end_pos = self.position_offset + current_local_pos;

            if end_pos > start_pos {
                // Only switch to unescaped mode if there's actually content to copy
                self.using_unescaped_buffer = true;
                let slice_start = start_pos.saturating_sub(self.position_offset);
                let slice_end = end_pos.saturating_sub(self.position_offset);

                // Ensure slice bounds are valid for the current data chunk
                if slice_start < data.len() && slice_end <= data.len() && slice_start <= slice_end {
                    let initial_part = &data[slice_start..slice_end];
                    for &byte in initial_part {
                        self.stream_buffer.append_unescaped_byte(byte)?;
                    }
                } else if slice_start < data.len() {
                    // Cross-chunk boundary case: copy what we can from current chunk
                    let actual_slice_end = data.len().min(slice_end);
                    let initial_part = &data[slice_start..actual_slice_end];
                    for &byte in initial_part {
                        self.stream_buffer.append_unescaped_byte(byte)?;
                    }
                }
                // If bounds are invalid, we're probably processing across chunk boundaries
                // The unescaped buffer is already marked as active, content will be added as we process more bytes
            }
        }
        Ok(())
    }

    /// Extract borrowed content from the input data
    pub fn extract_borrowed_content<'a>(&self, data: &'a [u8]) -> Result<&'a str, ParseError> {
        let start_pos = self.token_start_pos + 1;
        let end_pos = self.current_position;
        if end_pos >= start_pos {
            let s_bytes =
                &data[(start_pos - self.position_offset)..(end_pos - self.position_offset)];
            Ok(core::str::from_utf8(s_bytes)?)
        } else {
            Ok("")
        }
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
        self.start_content_token(pos);
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

    fn extract_string_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // PushParser handles content extraction through its own event emission pattern
        // This method is not used in the push parser flow - use extract_string_content_with_data instead
        Err(ParseError::Unexpected(crate::shared::UnexpectedState::StateMismatch))
    }

    fn extract_key_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // PushParser handles content extraction through its own event emission pattern
        // This method is not used in the push parser flow - use extract_key_content_with_data instead
        Err(ParseError::Unexpected(crate::shared::UnexpectedState::StateMismatch))
    }

    fn extract_key_content_new<'input, 'scratch>(
        &mut self,
        source: &impl DataSource<'input, 'scratch>,
        start_pos: usize,
    ) -> Result<Event<'input, 'scratch>, ParseError> {
        // Use the DataSource to get content based on whether we have unescaped content
        if source.has_unescaped_content() {
            // Get unescaped content from scratch buffer
            let content_bytes = source.get_unescaped_slice()?;
            let content_str = core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
            Ok(Event::Key(crate::String::Unescaped(content_str)))
        } else {
            // Get borrowed content from input
            let current_pos = self.current_position;
            let end_pos = current_pos;
            
            if end_pos >= start_pos {
                let content_bytes = source.get_borrowed_slice(start_pos + 1, end_pos)?; // +1 to skip quote
                let content_str = core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
                Ok(Event::Key(crate::String::Borrowed(content_str)))
            } else {
                // Empty key
                Ok(Event::Key(crate::String::Borrowed("")))
            }
        }
    }

    fn extract_number(
        &mut self,
        _start_pos: usize,
        _from_container_end: bool,
        _finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // PushParser handles content extraction through its own event emission pattern
        // This method is not used in the push parser flow - use extract_number_with_data instead
        Err(ParseError::Unexpected(crate::shared::UnexpectedState::StateMismatch))
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Process the collected unicode escape to UTF-8
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

        // Reset for next escape sequence
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

impl<H> PushContentBuilder<'_, H> {


    /// Get the handler and emit an event directly
    pub fn emit_event<E>(&mut self, event: Event<'_, '_>) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        self.handler.handle_event(event)
    }

    /// Clear the unescaped buffer
    #[allow(dead_code)]
    pub fn clear_unescaped(&mut self) {
        self.stream_buffer.clear_unescaped();
    }

    /// Validate and extract number (following ContentExtractor pattern)
    pub fn validate_and_extract_number_with_data<E>(&mut self, data: &[u8]) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let start_pos = match *self.parser_state() {
            State::Number(pos) => pos,
            _ => return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ).into()),
        };

        // Set state to None first to avoid borrowing conflicts
        *self.parser_state_mut() = State::None;
        
        // Extract and emit the number
        self.extract_number_with_data_impl(start_pos, data)
    }

    /// Extract number with data context (push parser specific implementation)
    fn extract_number_with_data_impl<E>(&mut self, start_pos: usize, data: &[u8]) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let json_number = if self.using_unescaped_buffer {
            // Number is in the unescaped buffer
            let number_bytes = self.stream_buffer.get_unescaped_slice().map_err(ParseError::from)?;
            JsonNumber::from_slice(number_bytes).map_err(E::from)?
        } else {
            // Number is in the input data, need to slice it  
            let current_pos = self.current_position;
            let end_pos = current_pos;
            
            if end_pos >= start_pos {
                let slice_start = start_pos.saturating_sub(self.position_offset);
                let slice_end = end_pos.saturating_sub(self.position_offset);
                
                if slice_end <= data.len() && slice_start <= slice_end {
                    let number_bytes = &data[slice_start..slice_end];
                    JsonNumber::from_slice(number_bytes).map_err(E::from)?
                } else {
                    return Err(ParseError::Unexpected(
                        crate::shared::UnexpectedState::InvalidSliceBounds,
                    ).into());
                }
            } else {
                return Err(ParseError::Unexpected(
                    crate::shared::UnexpectedState::InvalidSliceBounds,
                ).into());
            }
        };
        
        self.handler.handle_event(Event::Number(json_number))?;
        Ok(())
    }

    /// Handle unfinished number at end of parsing (ContentExtractor pattern)
    pub fn validate_and_extract_unfinished_number<E>(&mut self) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Validate parser state
        let _start_pos = match *self.parser_state() {
            State::Number(pos) => pos,
            _ => return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ).into()),
        };

        // For unfinished numbers, we should always be using the unescaped buffer
        if !self.using_unescaped_buffer {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ).into());
        }

        // Set state to None first to avoid borrowing conflicts
        *self.parser_state_mut() = State::None;

        // Extract number from unescaped buffer and emit event
        let number_bytes = self.stream_buffer.get_unescaped_slice().map_err(ParseError::from)?;
        let json_number = JsonNumber::from_slice(number_bytes).map_err(E::from)?;
        self.handler.handle_event(Event::Number(json_number))?;
        Ok(())
    }

    /// Validate and extract string content (following ContentExtractor pattern)
    pub fn validate_and_extract_string_content_with_data<E>(&mut self, data: &[u8]) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let start_pos = match *self.parser_state() {
            State::String(pos) => pos,
            _ => return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ).into()),
        };

        // Set state to None first to avoid borrowing conflicts
        *self.parser_state_mut() = State::None;
        
        // Extract and emit the string
        self.extract_string_content_with_data_impl(start_pos, data, false)
    }

    /// Validate and extract key content (following ContentExtractor pattern)
    pub fn validate_and_extract_key_content_with_data<E>(&mut self, data: &[u8]) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let start_pos = match *self.parser_state() {
            State::Key(pos) => pos,
            _ => return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ).into()),
        };

        // Set state to None first to avoid borrowing conflicts
        *self.parser_state_mut() = State::None;
        
        // Extract and emit the key
        self.extract_string_content_with_data_impl(start_pos, data, true)
    }

    /// Extract string or key content with data context (push parser specific implementation)
    fn extract_string_content_with_data_impl<E>(&mut self, _start_pos: usize, data: &[u8], is_key: bool) -> Result<(), E>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let event = if self.using_unescaped_buffer {
            // Content is in the unescaped buffer
            let content_bytes = self.stream_buffer.get_unescaped_slice().map_err(ParseError::from)?;
            let content_str = core::str::from_utf8(content_bytes).map_err(ParseError::InvalidUtf8)?;
            if is_key {
                Event::Key(crate::String::Unescaped(content_str))
            } else {
                Event::String(crate::String::Unescaped(content_str))
            }
        } else {
            // Content is in the input data, need to extract it
            let content_str = self.extract_borrowed_content(data).map_err(E::from)?;
            if is_key {
                Event::Key(crate::String::Borrowed(content_str))
            } else {
                Event::String(crate::String::Borrowed(content_str))
            }
        };
        
        self.handler.handle_event(event)?;
        self.queue_unescaped_reset();
        Ok(())
    }
}

/// A temporary DataSource implementation for PushParser that bridges the gap between
/// discrete input chunks and the unified DataSource interface.
///
/// This struct is designed to be created on-the-fly within PushParser::write() method
/// and provides access to both the current input chunk and the content builder's scratch buffer.
pub struct PushDataSource<'a, 'input, 'scratch, H> {
    /// Reference to the current input data chunk being processed
    input_chunk: &'input [u8],
    /// Reference to the content builder for accessing scratch buffer
    content_builder: &'a PushContentBuilder<'scratch, H>,
    /// Position offset to convert absolute positions to chunk-relative positions
    position_offset: usize,
}

impl<'a, 'input, 'scratch, H> PushDataSource<'a, 'input, 'scratch, H> {
    /// Create a new PushDataSource for the current input chunk
    ///
    /// # Arguments
    /// * `input_chunk` - The current data chunk being processed
    /// * `content_builder` - Reference to the content builder
    /// * `position_offset` - Absolute position where this chunk starts
    pub fn new(
        input_chunk: &'input [u8],
        content_builder: &'a PushContentBuilder<'scratch, H>,
        position_offset: usize,
    ) -> Self {
        Self {
            input_chunk,
            content_builder,
            position_offset,
        }
    }
}

impl<'a, 'input, 'scratch, H> DataSource<'input, 'scratch> for PushDataSource<'a, 'input, 'scratch, H> {
    fn get_borrowed_slice(&self, start: usize, end: usize) -> Result<&'input [u8], ParseError> {
        // Convert absolute positions to chunk-relative positions
        let chunk_start = start.saturating_sub(self.position_offset);
        let chunk_end = end.saturating_sub(self.position_offset);
        
        // Validate bounds within the current chunk
        if chunk_end > self.input_chunk.len() || chunk_start > chunk_end {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }
        
        Ok(&self.input_chunk[chunk_start..chunk_end])
    }

    fn get_unescaped_slice(&self) -> Result<&'scratch [u8], ParseError> {
        // Note: This has a similar lifetime issue as StreamContentBuilder
        // The content_builder reference has lifetime 'a, but we need to return
        // a reference with lifetime 'scratch. This would need careful lifetime
        // management in the actual implementation.
        // For now, this is a placeholder showing the intended interface.
        let _ = self.content_builder;
        Err(ParseError::Unexpected(
            crate::shared::UnexpectedState::StateMismatch,
        ))
    }

    fn has_unescaped_content(&self) -> bool {
        // Delegate to the content builder's state
        self.content_builder.using_unescaped_buffer
    }
}
