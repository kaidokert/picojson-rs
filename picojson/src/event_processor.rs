// SPDX-License-Identifier: Apache-2.0

//! Shared event processing logic between SliceParser and StreamParser.
//!
//! This module extracts the common event handling patterns to reduce code duplication
//! while preserving the performance characteristics of each parser type.

use crate::escape_processor::{EscapeProcessor, UnicodeEscapeCollector};
use crate::shared::{ContentRange, Event, ParserState, State, UnexpectedState};
use crate::ujson::{EventToken, Tokenizer};
use crate::{ujson, ParseError};

/// The core parser logic that handles the unified event processing loop.
///
/// This struct contains all the shared state and logic that was previously
/// duplicated between SliceParser and StreamParser. It uses trait abstractions
/// to handle the differences in content building and byte providing.
pub struct ParserCore<T: ujson::BitBucket, C: ujson::DepthCounter> {
    /// The tokenizer that processes JSON tokens
    pub tokenizer: Tokenizer<T, C>,
    /// Parser state and event storage
    pub parser_state: ParserState,
    /// Tracks if the parser is currently inside any escape sequence (\n, \uXXXX, etc.)
    in_escape_sequence: bool,
    /// Whether this parser handles chunked input (true for PushParser, false for Slice/Stream)
    /// When true, running out of input returns EndOfData. When false, calls tokenizer.finish().
    handles_chunked_input: bool,
}

impl<T: ujson::BitBucket, C: ujson::DepthCounter> ParserCore<T, C> {
    /// Create a new ParserCore for non-chunked parsers (SliceParser, StreamParser)
    pub fn new() -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            parser_state: ParserState::new(),
            in_escape_sequence: false,
            handles_chunked_input: false,
        }
    }

    /// Create a new ParserCore for chunked parsers (PushParser)
    pub fn new_chunked() -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            parser_state: ParserState::new(),
            in_escape_sequence: false,
            handles_chunked_input: true,
        }
    }

    /// Unified implementation with optional byte accumulation callback.
    /// This supports StreamParser-specific byte accumulation when no events are generated.
    /// SliceParser passes a no-op closure for byte_accumulator.
    pub fn next_event_impl<'a, P, F>(
        &mut self,
        provider: &'a mut P,
        escape_timing: EscapeTiming,
        byte_accumulator: F,
    ) -> Result<Event<'a, 'a>, ParseError>
    where
        P: ContentExtractor,
        F: FnMut(&mut P, u8) -> Result<(), ParseError>,
    {
        self.next_event_impl_with_flags(provider, escape_timing, byte_accumulator, false)
    }

    /// Extended version with flags for specialized behavior
    pub fn next_event_impl_with_flags<'a, P, F>(
        &mut self,
        provider: &'a mut P,
        escape_timing: EscapeTiming,
        mut byte_accumulator: F,
        always_accumulate_during_escapes: bool,
    ) -> Result<Event<'a, 'a>, ParseError>
    where
        P: ContentExtractor,
        F: FnMut(&mut P, u8) -> Result<(), ParseError>,
    {
        loop {
            while !have_events(&self.parser_state.evts) {
                if let Some(byte) = provider.next_byte()? {
                    {
                        clear_events(&mut self.parser_state.evts);
                        let mut callback = create_tokenizer_callback(&mut self.parser_state.evts);
                        self.tokenizer
                            .parse_chunk(&[byte], &mut callback)
                            .map_err(ParseError::TokenizerError)?;
                    }

                    let should_accumulate = if always_accumulate_during_escapes {
                        if self.in_escape_sequence {
                            true // Always accumulate during escape sequences
                        } else {
                            !have_events(&self.parser_state.evts) // Normal behavior outside escapes
                        }
                    } else {
                        !have_events(&self.parser_state.evts) && !self.in_escape_sequence
                    };

                    if should_accumulate {
                        byte_accumulator(provider, byte)?;
                    }
                } else {
                    // Handle end of input - behavior depends on parser type
                    if self.handles_chunked_input {
                        // For chunked parsers (PushParser), return EndOfData so they can handle chunk boundaries
                        return Err(ParseError::EndOfData);
                    } else {
                        // For non-chunked parsers (SliceParser, StreamParser), finish the document
                        {
                            let mut finish_callback =
                                create_tokenizer_callback(&mut self.parser_state.evts);
                            let _bytes_processed = self.tokenizer.finish(&mut finish_callback)?;
                        } // Drop the callback to release the borrow

                        // If finish() generated events, process them. Otherwise, return EndDocument.
                        if !have_events(&self.parser_state.evts) {
                            return Ok(Event::EndDocument);
                        }
                    }
                }
            }

            let taken_event = take_first_event(&mut self.parser_state.evts);
            let Some(taken) = taken_event else {
                return Err(UnexpectedState::StateMismatch.into());
            };

            // Try shared event processors first
            if let Some(result) =
                process_simple_events(&taken).or_else(|| provider.process_begin_events(&taken))
            {
                match result {
                    EventResult::Complete(event) => return Ok(event),
                    EventResult::ExtractString => return provider.validate_and_extract_string(),
                    EventResult::ExtractKey => return provider.validate_and_extract_key(),
                    EventResult::ExtractNumber(from_container_end) => {
                        return provider.validate_and_extract_number(from_container_end)
                    }
                    EventResult::Continue => continue,
                }
            }

            // Handle parser-specific events based on escape timing
            match taken {
                ujson::Event::Begin(EventToken::EscapeSequence) => {
                    self.in_escape_sequence = true;
                    provider.process_begin_escape_sequence_event()?;
                }
                ujson::Event::Begin(EventToken::UnicodeEscape) => {
                    self.in_escape_sequence = true;
                    provider.process_unicode_escape_events(&taken)?;
                }
                ujson::Event::End(EventToken::UnicodeEscape) => {
                    self.in_escape_sequence = false;
                    provider.process_unicode_escape_events(&taken)?;
                }
                ujson::Event::Begin(
                    escape_token @ (EventToken::EscapeQuote
                    | EventToken::EscapeBackslash
                    | EventToken::EscapeSlash
                    | EventToken::EscapeBackspace
                    | EventToken::EscapeFormFeed
                    | EventToken::EscapeNewline
                    | EventToken::EscapeCarriageReturn
                    | EventToken::EscapeTab),
                ) if escape_timing == EscapeTiming::OnBegin => {
                    // For SliceParser, the escape is handled in a single event.
                    // It begins and ends within this block.
                    self.in_escape_sequence = true;
                    provider.process_simple_escape_event(&escape_token)?;
                    self.in_escape_sequence = false;
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
                ) if escape_timing == EscapeTiming::OnEnd => {
                    // For StreamParser, the escape ends here.
                    provider.process_simple_escape_event(&escape_token)?;
                    self.in_escape_sequence = false;
                }
                _ => {
                    // All other events continue to next iteration
                }
            }
        }
    }
}

impl<T: ujson::BitBucket, C: ujson::DepthCounter> Default for ParserCore<T, C> {
    fn default() -> Self {
        Self::new()
    }
}

/// Enum to specify when escape sequences should be processed
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EscapeTiming {
    /// Process simple escape sequences on Begin events (SliceParser)
    OnBegin,
    /// Process simple escape sequences on End events (StreamParser)
    OnEnd,
}

/// Result of processing a tokenizer event
#[derive(Debug)]
pub enum EventResult<'a, 'b> {
    /// Event processing is complete, return this event to the user
    Complete(Event<'a, 'b>),
    /// Continue processing more tokenizer events
    Continue,
    /// Extract string content (delegate to parser-specific logic)
    ExtractString,
    /// Extract key content (delegate to parser-specific logic)
    ExtractKey,
    /// Extract number content (delegate to parser-specific logic)
    /// bool indicates if number was terminated by container delimiter
    ExtractNumber(bool),
}

/// Trait for content extraction operations that differ between parsers
/// Consolidates ParserContext and ContentExtractor functionality
pub trait ContentExtractor {
    /// Get the next byte from the input source
    /// Returns None when end of input is reached
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError>;

    /// Get current position in the input
    fn current_position(&self) -> usize;

    /// Begin string/key content processing at current position
    fn begin_string_content(&mut self, pos: usize);

    /// Get mutable access to parser state
    fn parser_state_mut(&mut self) -> &mut State;

    /// Get mutable access to the Unicode escape collector
    /// This eliminates the need for wrapper methods that just forward calls
    fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector;

    /// Extract string content using parser-specific logic
    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError>;

    /// Extract key content using parser-specific logic
    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError>;

    /// Extract a completed number using shared number parsing logic
    ///
    /// # Arguments
    /// * `start_pos` - Position where the number started
    /// * `from_container_end` - True if number was terminated by container delimiter
    /// * `finished` - True if the parser has finished processing input (StreamParser-specific)
    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError>;

    /// Shared validation and extraction for string content
    fn validate_and_extract_string(&mut self) -> Result<Event<'_, '_>, ParseError> {
        let start_pos = match *self.parser_state() {
            State::String(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the string
        if self
            .unicode_escape_collector_mut()
            .has_pending_high_surrogate()
        {
            return Err(ParseError::InvalidUnicodeCodepoint);
        }

        *self.parser_state_mut() = State::None;
        self.extract_string_content(start_pos)
    }

    /// Shared validation and extraction for key content
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
        self.extract_key_content(start_pos)
    }

    fn is_finished(&self) -> bool {
        false
    }

    /// Shared validation and extraction for number content
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

    /// Get the current parser state for escape context checking
    fn parser_state(&self) -> &State;

    /// Process Unicode escape sequence using shared collector logic
    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError>;

    /// Handle a simple escape character (after EscapeProcessor conversion)
    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError>;

    /// Begin escape sequence processing (lifecycle method with default no-op implementation)
    /// Called when escape sequence processing begins (e.g., on Begin(EscapeSequence))
    fn begin_escape_sequence(&mut self) -> Result<(), ParseError>;

    /// Begin unicode escape sequence processing
    fn begin_unicode_escape(&mut self) -> Result<(), ParseError>;

    /// Process Begin events that have similar patterns between parsers
    fn process_begin_events(
        &mut self,
        event: &ujson::Event,
    ) -> Option<EventResult<'static, 'static>> {
        match event {
            // String/Key Begin events - nearly identical patterns
            ujson::Event::Begin(EventToken::Key) => {
                let pos = self.current_position();
                *self.parser_state_mut() = State::Key(pos);
                self.begin_string_content(pos);
                Some(EventResult::Continue)
            }
            ujson::Event::Begin(EventToken::String) => {
                let pos = self.current_position();
                *self.parser_state_mut() = State::String(pos);
                self.begin_string_content(pos);
                Some(EventResult::Continue)
            }

            // Number Begin events - identical logic
            ujson::Event::Begin(
                EventToken::Number | EventToken::NumberAndArray | EventToken::NumberAndObject,
            ) => {
                let pos = self.current_position();
                let number_start = ContentRange::number_start_from_current(pos);
                *self.parser_state_mut() = State::Number(number_start);
                Some(EventResult::Continue)
            }

            // Primitive Begin events - identical logic
            ujson::Event::Begin(EventToken::True | EventToken::False | EventToken::Null) => {
                Some(EventResult::Continue)
            }

            _ => None,
        }
    }

    /// Process Begin(EscapeSequence) events using the enhanced lifecycle interface
    fn process_begin_escape_sequence_event(&mut self) -> Result<(), ParseError> {
        // Only process if we're inside a string or key
        match self.parser_state() {
            State::String(_) | State::Key(_) => {
                self.begin_escape_sequence()?;
            }
            _ => {} // Ignore if not in string/key context
        }
        Ok(())
    }

    /// Process simple escape sequence events that have similar patterns between parsers
    fn process_simple_escape_event(&mut self, escape_token: &EventToken) -> Result<(), ParseError> {
        // Clear any pending high surrogate state when we encounter a simple escape
        // This ensures that interrupted surrogate pairs (like \uD801\n\uDC37) are properly rejected
        self.unicode_escape_collector_mut().reset_all();

        // Use unified escape token processing from EscapeProcessor
        let unescaped_char = EscapeProcessor::process_escape_token(escape_token)?;

        // Only process if we're inside a string or key
        match self.parser_state() {
            State::String(_) | State::Key(_) => {
                self.handle_simple_escape_char(unescaped_char)?;
            }
            _ => {} // Ignore if not in string/key context
        }

        Ok(())
    }

    /// Process Unicode escape begin/end events that have similar patterns between parsers
    fn process_unicode_escape_events(&mut self, event: &ujson::Event) -> Result<bool, ParseError> {
        match event {
            ujson::Event::Begin(EventToken::UnicodeEscape) => {
                // Start Unicode escape collection - reset collector for new sequence
                // Only handle if we're inside a string or key
                match self.parser_state() {
                    State::String(_) | State::Key(_) => {
                        self.unicode_escape_collector_mut().reset();
                        self.begin_unicode_escape()?;
                    }
                    _ => {} // Ignore if not in string/key context
                }
                Ok(true) // Event was handled
            }
            ujson::Event::End(EventToken::UnicodeEscape) => {
                // Handle end of Unicode escape sequence (\uXXXX)
                match self.parser_state() {
                    State::String(_) | State::Key(_) => {
                        self.process_unicode_escape_with_collector()?;
                    }
                    _ => {} // Ignore if not in string/key context
                }
                Ok(true) // Event was handled
            }
            _ => Ok(false), // Event was not handled
        }
    }
}

/// Clear event storage array - utility function
pub fn clear_events(event_storage: &mut [Option<ujson::Event>; 2]) {
    event_storage[0] = None;
    event_storage[1] = None;
}

/// Creates a standard tokenizer callback for event storage
///
/// This callback stores tokenizer events in the parser's event array, filling the first
/// available slot. This pattern is identical across both SliceParser and StreamParser.
pub fn create_tokenizer_callback(
    event_storage: &mut [Option<ujson::Event>; 2],
) -> impl FnMut(ujson::Event, usize) + '_ {
    |event, _len| {
        for evt in event_storage.iter_mut() {
            if evt.is_none() {
                *evt = Some(event);
                return;
            }
        }
    }
}

/// Shared utility to check if any events are waiting to be processed
pub fn have_events(event_storage: &[Option<ujson::Event>; 2]) -> bool {
    event_storage.iter().any(|evt| evt.is_some())
}

/// Shared utility to extract the first available event from storage
pub fn take_first_event(event_storage: &mut [Option<ujson::Event>; 2]) -> Option<ujson::Event> {
    event_storage.iter_mut().find_map(|e| e.take())
}

/// Process simple container and primitive events that are identical between parsers
pub fn process_simple_events(event: &ujson::Event) -> Option<EventResult<'static, 'static>> {
    match event {
        // Container events - identical processing
        ujson::Event::ObjectStart => Some(EventResult::Complete(Event::StartObject)),
        ujson::Event::ObjectEnd => Some(EventResult::Complete(Event::EndObject)),
        ujson::Event::ArrayStart => Some(EventResult::Complete(Event::StartArray)),
        ujson::Event::ArrayEnd => Some(EventResult::Complete(Event::EndArray)),

        // Primitive values - identical processing
        ujson::Event::End(EventToken::True) => Some(EventResult::Complete(Event::Bool(true))),
        ujson::Event::End(EventToken::False) => Some(EventResult::Complete(Event::Bool(false))),
        ujson::Event::End(EventToken::Null) => Some(EventResult::Complete(Event::Null)),

        // Content extraction triggers - identical logic
        ujson::Event::End(EventToken::String) => Some(EventResult::ExtractString),
        ujson::Event::End(EventToken::Key) => Some(EventResult::ExtractKey),
        ujson::Event::End(EventToken::Number) => Some(EventResult::ExtractNumber(false)),
        ujson::Event::End(EventToken::NumberAndArray) => Some(EventResult::ExtractNumber(true)),
        ujson::Event::End(EventToken::NumberAndObject) => Some(EventResult::ExtractNumber(true)),

        // All other events need parser-specific handling
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_events() {
        assert!(matches!(
            process_simple_events(&ujson::Event::ObjectStart),
            Some(EventResult::Complete(Event::StartObject))
        ));

        assert!(matches!(
            process_simple_events(&ujson::Event::ArrayEnd),
            Some(EventResult::Complete(Event::EndArray))
        ));
    }

    #[test]
    fn test_primitive_events() {
        assert!(matches!(
            process_simple_events(&ujson::Event::End(EventToken::True)),
            Some(EventResult::Complete(Event::Bool(true)))
        ));

        assert!(matches!(
            process_simple_events(&ujson::Event::End(EventToken::Null)),
            Some(EventResult::Complete(Event::Null))
        ));
    }

    #[test]
    fn test_extraction_triggers() {
        assert!(matches!(
            process_simple_events(&ujson::Event::End(EventToken::String)),
            Some(EventResult::ExtractString)
        ));

        assert!(matches!(
            process_simple_events(&ujson::Event::End(EventToken::Number)),
            Some(EventResult::ExtractNumber(false))
        ));

        assert!(matches!(
            process_simple_events(&ujson::Event::End(EventToken::NumberAndArray)),
            Some(EventResult::ExtractNumber(true))
        ));
    }

    #[test]
    fn test_complex_events_not_handled() {
        assert!(process_simple_events(&ujson::Event::Begin(EventToken::String)).is_none());
        assert!(process_simple_events(&ujson::Event::Begin(EventToken::EscapeQuote)).is_none());
    }

    // Mock ContentExtractor for testing
    struct MockContentExtractor {
        position: usize,
        state: State,
        string_begin_calls: Vec<usize>,
    }

    impl MockContentExtractor {
        fn new() -> Self {
            Self {
                position: 42,
                state: State::None,
                string_begin_calls: Vec::new(),
            }
        }
    }

    impl ContentExtractor for MockContentExtractor {
        fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
            Ok(None)
        }

        fn current_position(&self) -> usize {
            self.position
        }

        fn begin_string_content(&mut self, pos: usize) {
            self.string_begin_calls.push(pos);
        }

        fn parser_state_mut(&mut self) -> &mut State {
            &mut self.state
        }

        fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
            unimplemented!("Mock doesn't need unicode collector")
        }

        fn extract_string_content(
            &mut self,
            _start_pos: usize,
        ) -> Result<Event<'_, '_>, ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn extract_key_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn extract_number(
            &mut self,
            _start_pos: usize,
            _from_container_end: bool,
            _finished: bool,
        ) -> Result<Event<'_, '_>, ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn parser_state(&self) -> &State {
            &self.state
        }

        fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
            Ok(())
        }

        fn handle_simple_escape_char(&mut self, _escape_char: u8) -> Result<(), ParseError> {
            Ok(())
        }

        fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
            Ok(())
        }

        fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
            Ok(())
        }
    }

    #[test]
    fn test_begin_events_key() {
        let mut context = MockContentExtractor::new();
        let event = ujson::Event::Begin(EventToken::Key);

        let result = context.process_begin_events(&event);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(context.state, State::Key(42)));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_string() {
        let mut context = MockContentExtractor::new();
        let event = ujson::Event::Begin(EventToken::String);

        let result = context.process_begin_events(&event);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(context.state, State::String(42)));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_number() {
        let mut context = MockContentExtractor::new();
        let event = ujson::Event::Begin(EventToken::Number);

        let result = context.process_begin_events(&event);

        assert!(matches!(result, Some(EventResult::Continue)));
        // Number should get position adjusted by ContentRange::number_start_from_current
        assert!(matches!(context.state, State::Number(_)));
        assert_eq!(context.string_begin_calls, Vec::<usize>::new()); // No string calls for numbers
    }

    #[test]
    fn test_begin_events_primitives() {
        let mut context = MockContentExtractor::new();

        for token in [EventToken::True, EventToken::False, EventToken::Null] {
            let event = ujson::Event::Begin(token);
            let result = context.process_begin_events(&event);
            assert!(matches!(result, Some(EventResult::Continue)));
        }

        // Should not affect state or string processing
        assert!(matches!(context.state, State::None));
        assert!(context.string_begin_calls.is_empty());
    }

    #[test]
    fn test_begin_events_not_handled() {
        let mut context = MockContentExtractor::new();
        let event = ujson::Event::Begin(EventToken::EscapeQuote);

        let result = context.process_begin_events(&event);

        assert!(result.is_none());
        assert!(matches!(context.state, State::None));
        assert!(context.string_begin_calls.is_empty());
    }

    #[test]
    fn test_tokenizer_callback() {
        let mut event_storage = [None, None];

        // Initially no events
        assert!(!have_events(&event_storage));

        {
            let mut callback = create_tokenizer_callback(&mut event_storage);

            // Add first event
            callback(ujson::Event::ObjectStart, 1);
        }
        assert!(have_events(&event_storage));
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_none());

        {
            let mut callback = create_tokenizer_callback(&mut event_storage);
            // Add second event
            callback(ujson::Event::ArrayStart, 1);
        }
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_some());

        {
            let mut callback = create_tokenizer_callback(&mut event_storage);
            // Storage is full, third event should be ignored (no panic)
            callback(ujson::Event::ObjectEnd, 1);
        }
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_some());
    }

    #[test]
    fn test_event_extraction() {
        let mut event_storage = [
            Some(ujson::Event::ObjectStart),
            Some(ujson::Event::ArrayStart),
        ];

        // Extract first event
        let first = take_first_event(&mut event_storage);
        assert!(matches!(first, Some(ujson::Event::ObjectStart)));
        assert!(event_storage[0].is_none());
        assert!(event_storage[1].is_some());

        // Extract second event
        let second = take_first_event(&mut event_storage);
        assert!(matches!(second, Some(ujson::Event::ArrayStart)));
        assert!(event_storage[0].is_none());
        assert!(event_storage[1].is_none());

        // No more events
        let none = take_first_event(&mut event_storage);
        assert!(none.is_none());
        assert!(!have_events(&event_storage));
    }
}
