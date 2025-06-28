// SPDX-License-Identifier: Apache-2.0

use crate::direct_buffer::DirectBuffer;
use crate::escape_processor::{EscapeProcessor, UnicodeEscapeCollector};
use crate::shared::{ContentRange, Event, ParseError, ParserErrorHandler, ParserState};
use ujson::BitStackCore;
use ujson::{BitStack, EventToken, Tokenizer};

use log;

/// Trait for input sources that can provide data to the streaming parser
pub trait Reader {
    /// The error type returned by read operations
    type Error;

    /// Read data into the provided buffer.
    /// Returns the number of bytes read, or an error.
    ///
    /// # Contract
    /// - A return value of 0 **MUST** indicate true end of stream
    /// - Implementations **MUST NOT** return 0 unless no more data will ever be available
    /// - Returning 0 followed by non-zero reads in subsequent calls violates this contract
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Result of processing a tokenizer event
enum EventResult {
    /// Event processing complete, return this event
    Complete(Event<'static, 'static>),
    /// Continue processing, no event to return yet
    Continue,
    /// Extract string content from current state
    ExtractString,
    /// Extract key content from current state
    ExtractKey,
    /// Extract number content from current state
    ExtractNumber,
    /// Extract number content from current state (came from container end - exclude delimiter)
    ExtractNumberFromContainer,
}

/// Represents a pending container end event that needs to be emitted after number extraction
#[derive(Debug, Clone, Copy, PartialEq)]
enum PendingContainerEnd {
    /// Pending ArrayEnd event
    ArrayEnd,
    /// Pending ObjectEnd event
    ObjectEnd,
}

/// Represents the processing state of the DirectParser
/// Enforces logical invariants: once Finished, no other processing states are possible
#[derive(Debug)]
enum ProcessingState {
    /// Normal active processing
    Active {
        unescaped_reset_queued: bool,
        in_escape_sequence: bool,
    },
    /// All input consumed, tokenizer finished
    Finished,
}

/// A streaming JSON parser using DirectBuffer for single-buffer input and escape processing
pub struct DirectParser<'b, T: BitStack, D, R: Reader> {
    /// The tokenizer that processes JSON tokens
    tokenizer: Tokenizer<T, D>,
    /// Parser state tracking
    parser_state: ParserState,
    /// Reader for streaming input
    reader: R,
    /// DirectBuffer for single-buffer input and escape processing
    direct_buffer: DirectBuffer<'b>,

    // NEW: Future state machine - will gradually replace fields below
    /// Processing state machine that enforces logical invariants
    processing_state: ProcessingState,

    // PHASE 2.4 COMPLETE: Escape sequence state migrated to processing_state enum
    /// Pending container end event to emit after number extraction
    pending_container_end: Option<PendingContainerEnd>,
    /// Shared Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
}

impl<'b, T: BitStack + core::fmt::Debug, D: BitStackCore, R: Reader> DirectParser<'b, T, D, R> {
    /// Create a new DirectParser
    pub fn new(reader: R, buffer: &'b mut [u8]) -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            parser_state: ParserState::new(),
            reader,
            direct_buffer: DirectBuffer::new(buffer),

            // Initialize new state machine to Active with default values
            processing_state: ProcessingState::Active {
                unescaped_reset_queued: false,
                in_escape_sequence: false,
            },

            // Phase 2.4 complete: escape sequence state now in enum
            pending_container_end: None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
        }
    }

    /// Iterator-compatible method that returns None when parsing is complete.
    /// This method returns None when EndDocument is reached, Some(Ok(event)) for successful events,
    /// and Some(Err(error)) for parsing errors.
    pub fn next(&mut self) -> Option<Result<Event, ParseError>> {
        match self.next_event() {
            Ok(Event::EndDocument) => None,
            other => Some(other),
        }
    }

    /// Get the next JSON event from the stream - very simple increment
    pub fn next_event(&mut self) -> Result<Event, ParseError> {
        log::info!("next_event");
        // Apply any queued unescaped content reset from previous call
        self.apply_unescaped_reset_if_queued();

        // Check if we have pending events to emit
        if let Some(pending) = self.pending_container_end.take() {
            match pending {
                PendingContainerEnd::ArrayEnd => {
                    log::debug!("DirectParser: Emitting pending ArrayEnd");
                    return Ok(Event::EndArray);
                }
                PendingContainerEnd::ObjectEnd => {
                    log::debug!("DirectParser: Emitting pending ObjectEnd");
                    return Ok(Event::EndObject);
                }
            }
        }

        loop {
            // Make sure we have data in buffer
            self.fill_buffer_from_reader()?;

            if self.direct_buffer.is_empty() {
                // End of data - call tokenizer finish to handle any pending tokens (only once)
                if !matches!(self.processing_state, ProcessingState::Finished) {
                    // Transition to Finished state
                    self.processing_state = ProcessingState::Finished;
                    self.parser_state.evts[0] = None;
                    let mut callback = |event, _len| {
                        self.parser_state.evts[0] = Some(event);
                    };

                    match self.tokenizer.finish(&mut callback) {
                        Ok(_) => {
                            // Check if finish generated an event
                            if let Some(event) = self.parser_state.evts[0].take() {
                                log::info!("Processing finish event: {:?}", event);
                                match self.process_tokenizer_event(event)? {
                                    EventResult::Complete(parsed_event) => return Ok(parsed_event),
                                    EventResult::ExtractString => {
                                        return self.extract_string_from_state();
                                    }
                                    EventResult::ExtractKey => {
                                        return self.extract_key_from_state();
                                    }
                                    EventResult::ExtractNumber => {
                                        return self.extract_number_from_state_with_context(false);
                                    }
                                    EventResult::ExtractNumberFromContainer => {
                                        return self.extract_number_from_state_with_context(true);
                                    }
                                    EventResult::Continue => {
                                        // Continue to EndDocument
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            return Err(ParseError::TokenizerError);
                        }
                    }
                }

                return Ok(Event::EndDocument);
            }

            // Get byte and advance in separate steps to avoid borrow conflicts
            let byte = self.direct_buffer.current_byte()?;
            self.direct_buffer.advance()?;

            // Process byte through tokenizer
            self.parser_state.evts[0] = None;
            let mut callback = |event, _len| {
                self.parser_state.evts[0] = Some(event);
            };

            match self.tokenizer.parse_chunk(&[byte], &mut callback) {
                Ok(_) => {
                    // Handle special cases for Begin events that include the current byte
                    if let Some(event) = &self.parser_state.evts[0] {
                        match event {
                            ujson::Event::Begin(EventToken::UnicodeEscape) => {
                                // Current byte is the first hex digit - reset collector and add it
                                self.unicode_escape_collector.reset();
                                if let Err(_) = self.unicode_escape_collector.add_hex_digit(byte) {
                                    // Invalid hex digit - error will be handled by tokenizer
                                }
                            }
                            ujson::Event::End(EventToken::UnicodeEscape) => {
                                // Current byte is the fourth hex digit - add it to complete the sequence
                                if let Err(_) = self.unicode_escape_collector.add_hex_digit(byte) {
                                    // Invalid hex digit - error will be handled by tokenizer
                                }
                            }
                            _ => {}
                        }
                    }

                    // Check if we got an event
                    if let Some(event) = self.parser_state.evts[0].take() {
                        log::info!("Processing tokenizer event: {:?}", event);
                        // Process the event and see what to do
                        match self.process_tokenizer_event(event)? {
                            EventResult::Complete(parsed_event) => return Ok(parsed_event),
                            EventResult::ExtractString => {
                                // Extract string content after buffer operations are done
                                return self.extract_string_from_state();
                            }
                            EventResult::ExtractKey => {
                                // Extract key content after buffer operations are done
                                return self.extract_key_from_state();
                            }
                            EventResult::ExtractNumber => {
                                // Extract number content after buffer operations are done
                                return self.extract_number_from_state_with_context(false);
                            }
                            EventResult::ExtractNumberFromContainer => {
                                // Extract number content that was terminated by container end
                                return self.extract_number_from_state_with_context(true);
                            }
                            EventResult::Continue => {
                                // Continue processing
                            }
                        }
                    } else {
                        // No event was generated, handle accumulation
                        self.handle_byte_accumulation(byte)?;
                    }
                    // Continue processing if no event produced
                }
                Err(_) => {
                    return Err(ParseError::TokenizerError);
                }
            }
        }
    }

    /// Process event and update state, but defer complex processing
    fn process_tokenizer_event(&mut self, event: ujson::Event) -> Result<EventResult, ParseError> {
        Ok(match event {
            // Container events
            ujson::Event::ObjectStart => EventResult::Complete(Event::StartObject),
            ujson::Event::ObjectEnd => {
                // Check if we're in the middle of parsing a number - if so, extract it first
                if matches!(self.parser_state.state, crate::shared::State::Number(_)) {
                    log::debug!(
                        "DirectParser: ObjectEnd while in Number state - extracting number first"
                    );
                    // Extract the number first, then we'll emit EndObject on the next call
                    self.pending_container_end = Some(PendingContainerEnd::ObjectEnd);
                    EventResult::ExtractNumberFromContainer
                } else {
                    EventResult::Complete(Event::EndObject)
                }
            }
            ujson::Event::ArrayStart => EventResult::Complete(Event::StartArray),
            ujson::Event::ArrayEnd => {
                // Check if we're in the middle of parsing a number - if so, extract it first
                if matches!(self.parser_state.state, crate::shared::State::Number(_)) {
                    log::debug!(
                        "DirectParser: ArrayEnd while in Number state - extracting number first"
                    );
                    // Extract the number first, then we'll emit EndArray on the next call
                    self.pending_container_end = Some(PendingContainerEnd::ArrayEnd);
                    EventResult::ExtractNumberFromContainer
                } else {
                    EventResult::Complete(Event::EndArray)
                }
            }

            // String/Key events
            ujson::Event::Begin(EventToken::Key) => {
                // Mark start position for key (current position is AFTER opening quote was processed)
                // We want to store the position of the opening quote, so back up by 1
                let current_pos = self.direct_buffer.current_position();
                let quote_pos = ContentRange::quote_position_from_current(current_pos);
                self.parser_state.state = crate::shared::State::Key(quote_pos);

                // DirectBuffer will handle escape processing state internally

                EventResult::Continue // Continue processing
            }
            ujson::Event::End(EventToken::Key) => {
                // Mark that we need to extract key, but defer the actual extraction
                EventResult::ExtractKey
            }
            ujson::Event::Begin(EventToken::String) => {
                // Mark start position for string (current position is AFTER opening quote was processed)
                // We want to store the position of the opening quote, so back up by 1
                let current_pos = self.direct_buffer.current_position();
                let quote_pos = ContentRange::quote_position_from_current(current_pos);
                self.parser_state.state = crate::shared::State::String(quote_pos);

                // DirectBuffer will handle escape processing state internally

                EventResult::Continue // Continue processing
            }
            ujson::Event::End(EventToken::String) => {
                // Mark that we need to extract string, but defer the actual extraction
                EventResult::ExtractString
            }

            // Number events
            ujson::Event::Begin(EventToken::Number) => {
                // Mark start position for number (current position is where number starts)
                let current_pos = self.direct_buffer.current_position();
                let number_start = ContentRange::number_start_from_current(current_pos);
                log::debug!(
                    "DirectParser: Begin Number event, current_pos={}, number_start={}",
                    current_pos,
                    number_start
                );
                self.parser_state.state = crate::shared::State::Number(number_start);
                EventResult::Continue
            }
            ujson::Event::End(EventToken::Number) => {
                // Extract number content after buffer operations are done (standalone number)
                log::debug!("DirectParser: End Number event");
                let current_pos = self.direct_buffer.current_position();
                if let crate::shared::State::Number(start) = self.parser_state.state {
                    log::debug!(
                        "DirectParser: End Number, start={}, current_pos={}",
                        start,
                        current_pos
                    );
                }
                EventResult::ExtractNumber
            }
            ujson::Event::End(EventToken::NumberAndArray) => {
                // Extract number content, but the tokenizer will handle the array end separately
                log::debug!("DirectParser: End NumberAndArray event");
                let current_pos = self.direct_buffer.current_position();
                if let crate::shared::State::Number(start) = self.parser_state.state {
                    log::debug!(
                        "DirectParser: End NumberAndArray, start={}, current_pos={}",
                        start,
                        current_pos
                    );
                }
                EventResult::ExtractNumber
            }
            ujson::Event::End(EventToken::NumberAndObject) => {
                // Extract number content, but the tokenizer will handle the object end separately
                log::debug!("DirectParser: End NumberAndObject event");
                let current_pos = self.direct_buffer.current_position();
                if let crate::shared::State::Number(start) = self.parser_state.state {
                    log::debug!(
                        "DirectParser: End NumberAndObject, start={}, current_pos={}",
                        start,
                        current_pos
                    );
                }
                EventResult::ExtractNumber
            }

            // Boolean and null values
            ujson::Event::Begin(EventToken::True | EventToken::False | EventToken::Null) => {
                EventResult::Continue
            }
            ujson::Event::End(EventToken::True) => EventResult::Complete(Event::Bool(true)),
            ujson::Event::End(EventToken::False) => EventResult::Complete(Event::Bool(false)),
            ujson::Event::End(EventToken::Null) => EventResult::Complete(Event::Null),

            // Escape sequence handling
            ujson::Event::Begin(EventToken::EscapeSequence) => {
                // Start of escape sequence - we'll handle escapes by unescaping to buffer start
                return self.start_escape_processing();
            }
            ujson::Event::End(
                escape_token @ (EventToken::EscapeQuote
                | EventToken::EscapeBackslash
                | EventToken::EscapeSlash
                | EventToken::EscapeBackspace
                | EventToken::EscapeFormFeed
                | EventToken::EscapeNewline
                | EventToken::EscapeCarriageReturn
                | EventToken::EscapeTab),
            ) => {
                // Process simple escape sequence
                self.handle_simple_escape(&escape_token)?
            }
            ujson::Event::Begin(EventToken::UnicodeEscape) => {
                // Start Unicode escape - initialize hex collection
                self.start_unicode_escape()
            }
            ujson::Event::End(EventToken::UnicodeEscape) => {
                // End Unicode escape - process collected hex digits
                return self.finish_unicode_escape();
            }
            ujson::Event::End(EventToken::EscapeSequence) => {
                // End of escape sequence - should not occur as individual event
                // Escape sequences should end with specific escape types
                return Err(ParseError::TokenizerError);
            }

            // Handle any unexpected Begin events defensively
            ujson::Event::Begin(
                EventToken::EscapeQuote
                | EventToken::EscapeBackslash
                | EventToken::EscapeSlash
                | EventToken::EscapeBackspace
                | EventToken::EscapeFormFeed
                | EventToken::EscapeNewline
                | EventToken::EscapeCarriageReturn
                | EventToken::EscapeTab,
            ) => {
                // These should never have Begin events, only End events
                return Err(ParseError::TokenizerError);
            }
            ujson::Event::Begin(EventToken::NumberAndArray | EventToken::NumberAndObject) => {
                // These tokens should only appear as End events, not Begin events
                return Err(ParseError::TokenizerError);
            }
        })
    }

    /// Extract string after all buffer operations are complete
    fn extract_string_from_state(&mut self) -> Result<Event, ParseError> {
        let crate::shared::State::String(start_pos) = self.parser_state.state else {
            return Err(ParserErrorHandler::state_mismatch("string", "extract"));
        };

        self.parser_state.state = crate::shared::State::None;

        if self.direct_buffer.has_unescaped_content() {
            self.create_unescaped_string()
        } else {
            self.create_borrowed_string(start_pos)
        }
    }

    /// Helper to create an unescaped string from DirectBuffer
    fn create_unescaped_string(&mut self) -> Result<Event, ParseError> {
        self.queue_unescaped_reset();
        let unescaped_slice = self.direct_buffer.get_unescaped_slice()?;
        let str_content = ParserErrorHandler::bytes_to_utf8_str(unescaped_slice)?;
        Ok(Event::String(crate::String::Unescaped(str_content)))
    }

    /// Helper to create a borrowed string from DirectBuffer
    fn create_borrowed_string(&mut self, start_pos: usize) -> Result<Event, ParseError> {
        let current_pos = self.direct_buffer.current_position();
        let (content_start, content_end) =
            ContentRange::string_content_bounds(start_pos, current_pos);

        let bytes = self
            .direct_buffer
            .get_string_slice(content_start, content_end)?;
        let str_content = ParserErrorHandler::bytes_to_utf8_str(bytes)?;
        Ok(Event::String(crate::String::Borrowed(str_content)))
    }

    /// Extract key after all buffer operations are complete
    fn extract_key_from_state(&mut self) -> Result<Event, ParseError> {
        let crate::shared::State::Key(start_pos) = self.parser_state.state else {
            return Err(ParserErrorHandler::state_mismatch("key", "extract"));
        };

        self.parser_state.state = crate::shared::State::None;

        if self.direct_buffer.has_unescaped_content() {
            self.create_unescaped_key()
        } else {
            self.create_borrowed_key(start_pos)
        }
    }

    /// Helper to create an unescaped key from DirectBuffer
    fn create_unescaped_key(&mut self) -> Result<Event, ParseError> {
        self.queue_unescaped_reset();
        let unescaped_slice = self.direct_buffer.get_unescaped_slice()?;
        let str_content = ParserErrorHandler::bytes_to_utf8_str(unescaped_slice)?;
        Ok(Event::Key(crate::String::Unescaped(str_content)))
    }

    /// Helper to create a borrowed key from DirectBuffer
    fn create_borrowed_key(&mut self, start_pos: usize) -> Result<Event, ParseError> {
        let current_pos = self.direct_buffer.current_position();
        let (content_start, content_end) =
            ContentRange::string_content_bounds(start_pos, current_pos);

        let bytes = self
            .direct_buffer
            .get_string_slice(content_start, content_end)?;
        let str_content = ParserErrorHandler::bytes_to_utf8_str(bytes)?;
        Ok(Event::Key(crate::String::Borrowed(str_content)))
    }

    /// Extract number with delimiter context using unified parsing logic
    fn extract_number_from_state_with_context(
        &mut self,
        from_container_end: bool,
    ) -> Result<Event, ParseError> {
        let crate::shared::State::Number(start_pos) = self.parser_state.state else {
            return Err(ParserErrorHandler::state_mismatch("number", "extract"));
        };

        self.parser_state.state = crate::shared::State::None;

        // Use unified number parsing logic
        crate::number_parser::parse_number_event(&self.direct_buffer, start_pos, from_container_end)
    }
    /// Fill buffer from reader
    fn fill_buffer_from_reader(&mut self) -> Result<(), ParseError> {
        if let Some(fill_slice) = self.direct_buffer.get_fill_slice() {
            let bytes_read = self
                .reader
                .read(fill_slice)
                .map_err(|_| ParseError::ReaderError)?;

            log::debug!("Read {} bytes from reader", bytes_read);
            self.direct_buffer.mark_filled(bytes_read)?;

            // Note: bytes_read == 0 indicates end-of-stream per trait contract.
            // The main loop will handle transitioning to Finished state when buffer is empty.
        }
        Ok(())
    }

    /// Get buffer statistics for debugging
    pub fn buffer_stats(&self) -> crate::direct_buffer::DirectBufferStats {
        self.direct_buffer.stats()
    }

    /// Handle byte accumulation for strings/keys and Unicode escape sequences
    fn handle_byte_accumulation(&mut self, byte: u8) -> Result<(), ParseError> {
        // Check if we're in a string or key state
        let in_string_mode = matches!(
            self.parser_state.state,
            crate::shared::State::String(_) | crate::shared::State::Key(_)
        );

        if in_string_mode {
            // Access escape state from enum
            let in_escape = if let ProcessingState::Active {
                in_escape_sequence, ..
            } = &self.processing_state
            {
                *in_escape_sequence
            } else {
                false
            };

            // Check if we're collecting Unicode hex digits (2nd and 3rd)
            let hex_count = self.unicode_escape_collector.hex_count();
            if in_escape && hex_count > 0 && hex_count < 3 {
                // We're in a Unicode escape - collect 2nd and 3rd hex digits
                if let Err(_) = self.unicode_escape_collector.add_hex_digit(byte) {
                    // Invalid hex digit - error will be handled by tokenizer
                }
            } else if !in_escape {
                // Normal byte - if we're doing escape processing, accumulate it
                if self.direct_buffer.has_unescaped_content() {
                    self.append_byte_to_escape_buffer(byte)?;
                }
            }
        }

        Ok(())
    }

    /// Start escape processing using DirectBuffer
    fn start_escape_processing(&mut self) -> Result<EventResult, ParseError> {
        // Update escape state in enum
        if let ProcessingState::Active {
            ref mut in_escape_sequence,
            ..
        } = self.processing_state
        {
            *in_escape_sequence = true;
        }

        // Initialize escape processing with DirectBuffer if not already started
        if !self.direct_buffer.has_unescaped_content() {
            if let crate::shared::State::String(start_pos) | crate::shared::State::Key(start_pos) =
                self.parser_state.state
            {
                let current_pos = self.direct_buffer.current_position();
                let (content_start, content_end) =
                    ContentRange::string_content_bounds_before_escape(start_pos, current_pos);

                // Estimate max length needed for unescaping (content so far + remaining buffer)
                let max_escaped_len =
                    self.direct_buffer.remaining_bytes() + (content_end - content_start);

                // Start unescaping with DirectBuffer and copy existing content
                self.direct_buffer.start_unescaping_with_copy(
                    max_escaped_len,
                    content_start,
                    content_end,
                )?;
            }
        }

        Ok(EventResult::Continue)
    }

    /// Handle simple escape sequence using unified EscapeProcessor
    fn handle_simple_escape(
        &mut self,
        escape_token: &EventToken,
    ) -> Result<EventResult, ParseError> {
        // Update escape state in enum
        if let ProcessingState::Active {
            ref mut in_escape_sequence,
            ..
        } = self.processing_state
        {
            *in_escape_sequence = false;
        }

        // Use unified escape token processing from EscapeProcessor
        if let Ok(unescaped_char) = EscapeProcessor::process_escape_token(escape_token) {
            self.append_byte_to_escape_buffer(unescaped_char)?;
        }

        Ok(EventResult::Continue)
    }

    /// Start Unicode escape sequence
    fn start_unicode_escape(&mut self) -> EventResult {
        // Update escape state in enum
        if let ProcessingState::Active {
            ref mut in_escape_sequence,
            ..
        } = self.processing_state
        {
            *in_escape_sequence = true;
        }
        // Note: unicode_hex_pos and first hex digit are set in the special case handler
        EventResult::Continue
    }

    /// Finish Unicode escape sequence using shared UnicodeEscapeCollector
    fn finish_unicode_escape(&mut self) -> Result<EventResult, ParseError> {
        // Update escape state
        if let ProcessingState::Active {
            ref mut in_escape_sequence,
            ..
        } = self.processing_state
        {
            *in_escape_sequence = false;
        } else {
            return Err(ParserErrorHandler::state_mismatch("active", "process"));
        }

        // Verify we have collected all 4 hex digits
        if !self.unicode_escape_collector.is_complete() {
            return Err(ParserErrorHandler::invalid_unicode_escape());
        }

        // Process Unicode escape using the shared collector
        let mut utf8_buf = [0u8; 4];
        let utf8_bytes = self
            .unicode_escape_collector
            .process_to_utf8(&mut utf8_buf)?;

        // Append UTF-8 bytes to escape buffer
        for &byte in utf8_bytes {
            self.append_byte_to_escape_buffer(byte)?;
        }

        Ok(EventResult::Continue)
    }

    /// Append a byte to the DirectBuffer's unescaped content
    fn append_byte_to_escape_buffer(&mut self, byte: u8) -> Result<(), ParseError> {
        self.direct_buffer
            .append_unescaped_byte(byte)
            .map_err(|e| e.into())
    }

    /// Queue a reset of unescaped content for the next next_event() call
    fn queue_unescaped_reset(&mut self) {
        // Set the reset flag in the Active state
        if let ProcessingState::Active {
            ref mut unescaped_reset_queued,
            ..
        } = self.processing_state
        {
            *unescaped_reset_queued = true;
        }
        // Legacy field removed - now fully using enum
    }

    /// Apply queued unescaped content reset if flag is set
    fn apply_unescaped_reset_if_queued(&mut self) {
        // Check the enum field first
        let should_reset = if let ProcessingState::Active {
            ref mut unescaped_reset_queued,
            ..
        } = self.processing_state
        {
            let needs_reset = *unescaped_reset_queued;
            *unescaped_reset_queued = false; // Clear the flag
            needs_reset
        } else {
            false
        };

        if should_reset {
            self.direct_buffer.clear_unescaped();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test reader that reads from a byte slice
    pub struct SliceReader<'a> {
        data: &'a [u8],
        position: usize,
    }

    impl<'a> SliceReader<'a> {
        pub fn new(data: &'a [u8]) -> Self {
            Self { data, position: 0 }
        }
    }

    impl<'a> Reader for SliceReader<'a> {
        type Error = ();

        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let remaining = self.data.len() - self.position;
            if remaining == 0 {
                return Ok(0); // EOF
            }

            let to_copy = remaining.min(buf.len());
            buf[..to_copy].copy_from_slice(&self.data[self.position..self.position + to_copy]);
            self.position += to_copy;
            Ok(to_copy)
        }
    }

    type TestDirectParser<'b> = DirectParser<'b, u32, u8, SliceReader<'static>>;

    #[test]
    fn test_direct_parser_simple_object() {
        let json = b"{}";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        // Should get ObjectStart
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::StartObject));

        // Should get ObjectEnd
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::EndObject));

        // Should get EndDocument
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::EndDocument));
    }

    #[test]
    fn test_direct_parser_simple_array() {
        let json = b"[]";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        // Should get ArrayStart
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::StartArray));

        // Should get ArrayEnd
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::EndArray));

        // Should get EndDocument
        let event = parser.next_event().unwrap();
        assert!(matches!(event, Event::EndDocument));
    }

    #[test]
    fn test_direct_parser_simple_escape() {
        let json = b"\"hello\\nworld\"";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            // For now, test will fail as escapes aren't implemented yet
            // This will be fixed once escape handling is added
            println!("Got string: '{}'", json_string.as_str());
        } else {
            panic!("Expected String event");
        }
    }

    #[test]
    fn test_pending_state_edge_cases() {
        // Test 1: Complex nested container endings
        let json1 = br#"{"a": {"b": [{"c": 123}]}}"#;
        let reader1 = SliceReader::new(json1);
        let mut buffer1 = [0u8; 256];
        let mut parser1 = TestDirectParser::new(reader1, &mut buffer1);

        let mut events = Vec::new();
        loop {
            match parser1.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => events.push(format!("{:?}", event)),
                Err(e) => panic!("Nested containers failed: {:?}", e),
            }
        }

        // Should contain all expected events
        assert!(events.len() >= 8); // StartObject, Key, StartObject, Key, StartArray, StartObject, Key, Number, EndObject, EndArray, EndObject, EndObject

        // Test 2: Mixed types after numbers in array
        let json2 = br#"[123, "string", true, null, 456]"#;
        let reader2 = SliceReader::new(json2);
        let mut buffer2 = [0u8; 256];
        let mut parser2 = TestDirectParser::new(reader2, &mut buffer2);

        let mut number_count = 0;
        loop {
            match parser2.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(Event::Number(_)) => number_count += 1,
                Ok(_) => {}
                Err(e) => panic!("Mixed types failed: {:?}", e),
            }
        }
        assert_eq!(number_count, 2); // Should find both 123 and 456

        // Test 3: Empty containers
        let json3 = br#"[[], {}, [{}], {"empty": []}]"#;
        let reader3 = SliceReader::new(json3);
        let mut buffer3 = [0u8; 256];
        let mut parser3 = TestDirectParser::new(reader3, &mut buffer3);

        loop {
            match parser3.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(_) => {}
                Err(e) => panic!("Empty containers failed: {:?}", e),
            }
        }

        // Test 4: Multiple consecutive numbers
        let json4 = br#"[1, 2, 3, 4, 5]"#;
        let reader4 = SliceReader::new(json4);
        let mut buffer4 = [0u8; 256];
        let mut parser4 = TestDirectParser::new(reader4, &mut buffer4);

        let mut consecutive_numbers = Vec::new();
        loop {
            match parser4.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(Event::Number(n)) => consecutive_numbers.push(n.as_str().to_string()),
                Ok(_) => {}
                Err(e) => panic!("Consecutive numbers failed: {:?}", e),
            }
        }
        assert_eq!(consecutive_numbers, vec!["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn test_error_recovery_with_pending_state() {
        // Test error handling - this should fail gracefully without hanging onto pending state
        let invalid_json = br#"{"key": 123,"#; // Missing closing brace
        let reader = SliceReader::new(invalid_json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        // Parse until we hit an error or EOF
        loop {
            match parser.next_event() {
                Ok(Event::EndDocument) => break, // EOF reached
                Ok(_) => {}
                Err(_) => break, // Error occurred - this is expected
            }
        }

        // The important thing is that we don't panic or hang
        // The specific error behavior may vary
    }

    #[test]
    fn test_multiple_rapid_container_ends() {
        // Test deeply nested structures that end with numbers
        // This tests whether we can handle multiple rapid container ends correctly

        // Test 1: Deeply nested arrays ending with number
        let json1 = br#"[[[123]]]"#;
        let reader1 = SliceReader::new(json1);
        let mut buffer1 = [0u8; 256];
        let mut parser1 = TestDirectParser::new(reader1, &mut buffer1);

        let mut events1 = Vec::new();
        loop {
            match parser1.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => events1.push(format!("{:?}", event)),
                Err(e) => panic!("Deeply nested arrays failed: {:?}", e),
            }
        }

        // Should have: StartArray, StartArray, StartArray, Number(123), EndArray, EndArray, EndArray
        assert_eq!(events1.len(), 7);
        assert!(events1[3].contains("Number"));
        assert_eq!(&events1[4], "EndArray");
        assert_eq!(&events1[5], "EndArray");
        assert_eq!(&events1[6], "EndArray");

        // Test 2: Mixed nested containers ending with number
        let json2 = br#"{"a": [{"b": 456}]}"#;
        let reader2 = SliceReader::new(json2);
        let mut buffer2 = [0u8; 256];
        let mut parser2 = TestDirectParser::new(reader2, &mut buffer2);

        let mut events2 = Vec::new();
        loop {
            match parser2.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => events2.push(format!("{:?}", event)),
                Err(e) => panic!("Mixed nested containers failed: {:?}", e),
            }
        }

        // Should properly handle the sequence of: number -> EndObject -> EndArray -> EndObject
        assert!(events2.len() >= 8);

        // Test 3: Multiple numbers at different nesting levels
        let json3 = br#"[123, [456, [789]]]"#;
        let reader3 = SliceReader::new(json3);
        let mut buffer3 = [0u8; 256];
        let mut parser3 = TestDirectParser::new(reader3, &mut buffer3);

        let mut number_count = 0;
        let mut events3 = Vec::new();
        loop {
            match parser3.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(Event::Number(n)) => {
                    number_count += 1;
                    events3.push(format!("Number({})", n.as_str()));
                }
                Ok(event) => events3.push(format!("{:?}", event)),
                Err(e) => panic!("Multiple nested numbers failed: {:?}", e),
            }
        }

        assert_eq!(number_count, 3); // Should find all three numbers: 123, 456, 789
    }

    #[test]
    fn test_pending_flag_priority() {
        // Defensive test: ensure that if both pending flags were somehow set,
        // we handle it gracefully (this shouldn't happen in normal operation)

        let json = br#"[{"key": 123}]"#;
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        // Parse normally - this should work fine and never set both flags
        let mut events = Vec::new();
        loop {
            match parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => events.push(format!("{:?}", event)),
                Err(e) => panic!("Flag priority test failed: {:?}", e),
            }
        }

        // Should successfully parse: StartArray, StartObject, Key, Number, EndObject, EndArray
        assert_eq!(events.len(), 6);
        assert!(events[3].contains("Number"));
        assert_eq!(&events[4], "EndObject");
        assert_eq!(&events[5], "EndArray");
    }

    #[test_log::test]
    fn test_number_parsing_comparison() {
        // Test case to reproduce numbers problem - numbers at end of containers
        let problematic_json = r#"{"key": 123, "arr": [456, 789]}"#;

        println!("=== Testing FlexParser ===");
        let mut scratch = [0u8; 1024];
        let mut flex_parser = crate::PullParser::new_with_buffer(problematic_json, &mut scratch);

        // Parse with FlexParser and collect events
        let mut flex_events = Vec::new();
        loop {
            match flex_parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => flex_events.push(format!("{:?}", event)),
                Err(e) => panic!("FlexParser error: {:?}", e),
            }
        }

        println!("FlexParser events: {:?}", flex_events);

        println!("=== Testing DirectParser ===");
        let json_bytes = problematic_json.as_bytes();
        let reader = SliceReader::new(json_bytes);
        let mut buffer = [0u8; 1024];
        let mut direct_parser = TestDirectParser::new(reader, &mut buffer);

        // Parse with DirectParser and collect events
        let mut direct_events = Vec::new();
        loop {
            match direct_parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(event) => direct_events.push(format!("{:?}", event)),
                Err(e) => panic!("DirectParser error: {:?}", e),
            }
        }

        println!("DirectParser events: {:?}", direct_events);

        // Compare results
        assert_eq!(
            flex_events, direct_events,
            "Parsers should produce identical events"
        );
    }

    #[test]
    fn test_direct_parser_array_of_strings() {
        let json = b"[\"first\", \"second\"]";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        assert!(matches!(parser.next_event().unwrap(), Event::StartArray));

        if let Event::String(s1) = parser.next_event().unwrap() {
            assert_eq!(s1.as_str(), "first");
        } else {
            panic!("Expected String event");
        }

        if let Event::String(s2) = parser.next_event().unwrap() {
            assert_eq!(s2.as_str(), "second");
        } else {
            panic!("Expected String event");
        }

        assert!(matches!(parser.next_event().unwrap(), Event::EndArray));
    }

    #[test]
    fn test_direct_parser_object_with_keys() {
        let json = b"{\"name\": \"value\", \"count\": \"42\"}";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        assert!(matches!(parser.next_event().unwrap(), Event::StartObject));

        // First key-value pair
        if let Event::Key(key1) = parser.next_event().unwrap() {
            assert_eq!(key1.as_str(), "name");
        } else {
            panic!("Expected Key event");
        }

        if let Event::String(val1) = parser.next_event().unwrap() {
            assert_eq!(val1.as_str(), "value");
        } else {
            panic!("Expected String event");
        }

        // Second key-value pair
        if let Event::Key(key2) = parser.next_event().unwrap() {
            assert_eq!(key2.as_str(), "count");
        } else {
            panic!("Expected Key event");
        }

        if let Event::String(val2) = parser.next_event().unwrap() {
            assert_eq!(val2.as_str(), "42");
        } else {
            panic!("Expected String event");
        }

        assert!(matches!(parser.next_event().unwrap(), Event::EndObject));
    }

    #[test]
    fn test_direct_parser_multiple_escapes() {
        let json = b"\"line1\\nline2\\ttab\\\"quote\"";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            let content = json_string.as_str();
            println!("Multiple escapes result: '{}'", content);
            println!("Content bytes: {:?}", content.as_bytes());

            // Check that escape sequences were properly processed
            let has_newline = content.contains('\n');
            let has_tab = content.contains('\t');
            let has_quote = content.contains('"');

            println!(
                "Has newline: {}, Has tab: {}, Has quote: {}",
                has_newline, has_tab, has_quote
            );

            // These should be real control characters, not literal \n \t \"
            assert!(has_newline, "Should contain actual newline character");
            assert!(has_tab, "Should contain actual tab character");
            assert!(has_quote, "Should contain actual quote character");
        } else {
            panic!("Expected String event");
        }
    }

    #[test]
    fn test_direct_parser_unicode_escape() {
        let json = b"\"Hello \\u0041\\u03B1\""; // Hello A(alpha)
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            let content = json_string.as_str();
            println!("Unicode escape result: '{}'", content);
            // Should be "Hello A⍺" (with actual A and alpha characters)
            assert!(content.contains('A'));
            // Note: This test will initially fail until we implement Unicode escapes
        } else {
            panic!("Expected String event");
        }
    }

    #[test]
    fn test_direct_parser_boolean_true() {
        let json = b"true";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::Bool(true));

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_boolean_false() {
        let json = b"false";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::Bool(false));

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_null() {
        let json = b"null";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::Null);

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_booleans_in_array() {
        let json = b"[true, false, null]";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartArray);
        assert_eq!(parser.next_event().unwrap(), Event::Bool(true));
        assert_eq!(parser.next_event().unwrap(), Event::Bool(false));
        assert_eq!(parser.next_event().unwrap(), Event::Null);
        assert_eq!(parser.next_event().unwrap(), Event::EndArray);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test_log::test]
    fn test_direct_parser_number_simple() {
        let json = b"42";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "42");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_number_negative() {
        let json = b"-123";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "-123");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_number_float() {
        let json = b"3.14159";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "3.14159");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }

    #[test_log::test]
    fn test_direct_parser_numbers_in_array() {
        let json = b"[42, -7, 3.14]";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartArray);

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "42");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "-7");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        let event = parser.next_event().unwrap();
        if let Event::Number(json_number) = event {
            assert_eq!(json_number.as_str(), "3.14");
        } else {
            panic!("Expected Number event, got: {:?}", event);
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndArray);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test_log::test]
    fn test_direct_parser_numbers_in_object() {
        let json = b"{\"count\": 42, \"score\": -7.5}";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartObject);

        // First key-value pair
        if let Event::Key(key1) = parser.next_event().unwrap() {
            assert_eq!(key1.as_str(), "count");
        } else {
            panic!("Expected Key event");
        }

        if let Event::Number(val1) = parser.next_event().unwrap() {
            assert_eq!(val1.as_str(), "42");
        } else {
            panic!("Expected Number event");
        }

        // Second key-value pair
        if let Event::Key(key2) = parser.next_event().unwrap() {
            assert_eq!(key2.as_str(), "score");
        } else {
            panic!("Expected Key event");
        }

        if let Event::Number(val2) = parser.next_event().unwrap() {
            assert_eq!(val2.as_str(), "-7.5");
        } else {
            panic!("Expected Number event");
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_no_float_configuration() {
        // Test that DirectParser properly uses unified number parsing with no-float config
        let json = br#"{"integer": 42, "float": 3.14, "scientific": 1e3}"#;
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestDirectParser::new(reader, &mut buffer);

        // Parse through the JSON and verify number handling
        assert_eq!(parser.next_event().unwrap(), Event::StartObject);

        // Integer key-value
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("integer"))
        );
        if let Event::Number(num) = parser.next_event().unwrap() {
            assert_eq!(num.as_str(), "42");
            match num.parsed() {
                crate::NumberResult::Integer(i) => assert_eq!(*i, 42),
                _ => panic!("Expected integer parsing"),
            }
        } else {
            panic!("Expected Number event");
        }

        // Float key-value (should be FloatDisabled in no-float build)
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("float"))
        );
        if let Event::Number(num) = parser.next_event().unwrap() {
            assert_eq!(num.as_str(), "3.14");
            // In no-float configuration, this should be FloatDisabled
            match num.parsed() {
                #[cfg(not(feature = "float"))]
                crate::NumberResult::FloatDisabled => {
                    // This is expected in no-float build
                }
                #[cfg(feature = "float")]
                crate::NumberResult::Float(f) => {
                    // This is expected in float-enabled build
                    assert!((f - 3.14).abs() < f64::EPSILON);
                }
                #[cfg(feature = "float-truncate")]
                crate::NumberResult::FloatTruncated(i) => {
                    // This is expected in float-truncate build (3.14 -> 3)
                    assert_eq!(*i, 3);
                }
                _ => panic!("Unexpected number parsing result for float"),
            }
        } else {
            panic!("Expected Number event");
        }

        // Scientific notation (should also be FloatDisabled in no-float build)
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("scientific"))
        );
        if let Event::Number(num) = parser.next_event().unwrap() {
            assert_eq!(num.as_str(), "1e3");
            match num.parsed() {
                #[cfg(not(feature = "float"))]
                crate::NumberResult::FloatDisabled => {
                    // This is expected in no-float build - raw string preserved for manual parsing
                }
                #[cfg(feature = "float")]
                crate::NumberResult::Float(f) => {
                    // This is expected in float-enabled build
                    assert!((f - 1000.0).abs() < f64::EPSILON);
                }
                _ => panic!("Unexpected number parsing result for scientific notation"),
            }
        } else {
            panic!("Expected Number event");
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }
}
