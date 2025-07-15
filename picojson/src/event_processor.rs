// SPDX-License-Identifier: Apache-2.0

//! Shared event processing logic between SliceParser and StreamParser.
//!
//! This module extracts the common event handling patterns to reduce code duplication
//! while preserving the performance characteristics of each parser type.

use crate::shared::{ContentRange, State};
use crate::ujson::EventToken;
use crate::{Event, ParseError};

/// Escape handling trait for abstracting escape sequence processing between parsers
pub trait EscapeHandler {
    /// Get the current parser state for escape context checking
    fn parser_state(&self) -> &crate::shared::State;

    /// Process Unicode escape sequence using shared collector logic
    fn process_unicode_escape_with_collector(&mut self) -> Result<(), crate::ParseError>;

    /// Handle a simple escape character (after EscapeProcessor conversion)
    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), crate::ParseError>;

    /// Begin escape sequence processing (lifecycle method with default no-op implementation)
    /// Called when escape sequence processing begins (e.g., on Begin(EscapeSequence))
    fn begin_escape_sequence(&mut self) -> Result<(), crate::ParseError> {
        Ok(())
    }

    /// Begin unicode escape sequence processing
    /// Default implementation is no-op - suitable for parsers that don't need special handling
    fn begin_unicode_escape(&mut self) -> Result<(), crate::ParseError> {
        Ok(())
    }
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

/// Process Begin events that have similar patterns between parsers
pub fn process_begin_events<C: ContentExtractor>(
    event: &crate::ujson::Event,
    content_extractor: &mut C,
) -> Option<EventResult<'static, 'static>> {
    match event {
        // String/Key Begin events - nearly identical patterns
        crate::ujson::Event::Begin(EventToken::Key) => {
            let pos = content_extractor.current_position();
            *content_extractor.parser_state_mut() = State::Key(pos);
            content_extractor.begin_string_content(pos);
            Some(EventResult::Continue)
        }
        crate::ujson::Event::Begin(EventToken::String) => {
            let pos = content_extractor.current_position();
            *content_extractor.parser_state_mut() = State::String(pos);
            content_extractor.begin_string_content(pos);
            Some(EventResult::Continue)
        }

        // Number Begin events - identical logic
        crate::ujson::Event::Begin(
            EventToken::Number | EventToken::NumberAndArray | EventToken::NumberAndObject,
        ) => {
            let pos = content_extractor.current_position();
            let number_start = ContentRange::number_start_from_current(pos);
            *content_extractor.parser_state_mut() = State::Number(number_start);
            Some(EventResult::Continue)
        }

        // Primitive Begin events - identical logic
        crate::ujson::Event::Begin(EventToken::True | EventToken::False | EventToken::Null) => {
            Some(EventResult::Continue)
        }

        _ => None,
    }
}

/// Clear event storage array - utility function
pub fn clear_events(event_storage: &mut [Option<crate::ujson::Event>; 2]) {
    event_storage[0] = None;
    event_storage[1] = None;
}

/// Trait for content extraction operations that differ between parsers
/// Consolidates ParserContext and ContentExtractor functionality
pub trait ContentExtractor: EscapeHandler {
    /// Get current position in the input
    fn current_position(&self) -> usize;

    /// Begin string/key content processing at current position
    fn begin_string_content(&mut self, pos: usize);

    /// Get mutable access to parser state
    fn parser_state_mut(&mut self) -> &mut crate::shared::State;

    /// Get mutable access to the Unicode escape collector
    /// This eliminates the need for wrapper methods that just forward calls
    fn unicode_escape_collector_mut(
        &mut self,
    ) -> &mut crate::escape_processor::UnicodeEscapeCollector;

    /// Extract string content using parser-specific logic
    fn extract_string_content(
        &mut self,
        start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError>;

    /// Extract key content using parser-specific logic
    fn extract_key_content(
        &mut self,
        start_pos: usize,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError>;

    /// Extract number content using parser-specific logic
    fn extract_number_content(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError>;

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
    ) -> Result<crate::Event<'_, '_>, crate::ParseError>;

    /// Shared validation and extraction for string content
    fn validate_and_extract_string(&mut self) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        let start_pos = match *self.parser_state() {
            crate::shared::State::String(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the string
        if self
            .unicode_escape_collector_mut()
            .has_pending_high_surrogate()
        {
            return Err(crate::ParseError::InvalidUnicodeCodepoint);
        }

        *self.parser_state_mut() = crate::shared::State::None;
        self.extract_string_content(start_pos)
    }

    /// Shared validation and extraction for key content
    fn validate_and_extract_key(&mut self) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        let start_pos = match *self.parser_state() {
            crate::shared::State::Key(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the key
        if self
            .unicode_escape_collector_mut()
            .has_pending_high_surrogate()
        {
            return Err(crate::ParseError::InvalidUnicodeCodepoint);
        }

        *self.parser_state_mut() = crate::shared::State::None;
        self.extract_key_content(start_pos)
    }

    /// Shared validation and extraction for number content
    fn validate_and_extract_number(
        &mut self,
        from_container_end: bool,
    ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        let start_pos = match *self.parser_state() {
            crate::shared::State::Number(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        *self.parser_state_mut() = crate::shared::State::None;
        self.extract_number_content(start_pos, from_container_end)
    }
}

/// Creates a standard tokenizer callback for event storage
///
/// This callback stores tokenizer events in the parser's event array, filling the first
/// available slot. This pattern is identical across both SliceParser and StreamParser.
pub fn create_tokenizer_callback(
    event_storage: &mut [Option<crate::ujson::Event>; 2],
) -> impl FnMut(crate::ujson::Event, usize) + '_ {
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
pub fn have_events(event_storage: &[Option<crate::ujson::Event>; 2]) -> bool {
    event_storage.iter().any(|evt| evt.is_some())
}

/// Shared utility to extract the first available event from storage
pub fn take_first_event(
    event_storage: &mut [Option<crate::ujson::Event>; 2],
) -> Option<crate::ujson::Event> {
    event_storage.iter_mut().find_map(|e| e.take())
}

/// Process Begin(EscapeSequence) events using the enhanced lifecycle interface
pub fn process_begin_escape_sequence_event<H: EscapeHandler>(
    handler: &mut H,
) -> Result<(), crate::ParseError> {
    // Only process if we're inside a string or key
    match handler.parser_state() {
        crate::shared::State::String(_) | crate::shared::State::Key(_) => {
            handler.begin_escape_sequence()?;
        }
        _ => {} // Ignore if not in string/key context
    }
    Ok(())
}

/// Process simple escape sequence events that have similar patterns between parsers
pub fn process_simple_escape_event<C: ContentExtractor>(
    escape_token: &EventToken,
    content_extractor: &mut C,
) -> Result<(), crate::ParseError> {
    // Clear any pending high surrogate state when we encounter a simple escape
    // This ensures that interrupted surrogate pairs (like \uD801\n\uDC37) are properly rejected
    content_extractor.unicode_escape_collector_mut().reset_all();

    // Use unified escape token processing from EscapeProcessor
    let unescaped_char =
        crate::escape_processor::EscapeProcessor::process_escape_token(escape_token)?;

    // Only process if we're inside a string or key
    match content_extractor.parser_state() {
        crate::shared::State::String(_) | crate::shared::State::Key(_) => {
            content_extractor.handle_simple_escape_char(unescaped_char)?;
        }
        _ => {} // Ignore if not in string/key context
    }

    Ok(())
}

/// Process Unicode escape begin/end events that have similar patterns between parsers
pub fn process_unicode_escape_events<C: ContentExtractor>(
    event: &crate::ujson::Event,
    content_extractor: &mut C,
) -> Result<bool, crate::ParseError> {
    match event {
        crate::ujson::Event::Begin(EventToken::UnicodeEscape) => {
            // Start Unicode escape collection - reset collector for new sequence
            // Only handle if we're inside a string or key
            match content_extractor.parser_state() {
                crate::shared::State::String(_) | crate::shared::State::Key(_) => {
                    content_extractor.unicode_escape_collector_mut().reset();
                    content_extractor.begin_unicode_escape()?;
                }
                _ => {} // Ignore if not in string/key context
            }
            Ok(true) // Event was handled
        }
        crate::ujson::Event::End(EventToken::UnicodeEscape) => {
            // Handle end of Unicode escape sequence (\uXXXX)
            match content_extractor.parser_state() {
                crate::shared::State::String(_) | crate::shared::State::Key(_) => {
                    content_extractor.process_unicode_escape_with_collector()?;
                }
                _ => {} // Ignore if not in string/key context
            }
            Ok(true) // Event was handled
        }
        _ => Ok(false), // Event was not handled
    }
}

/// Process simple container and primitive events that are identical between parsers
pub fn process_simple_events(event: &crate::ujson::Event) -> Option<EventResult<'static, 'static>> {
    match event {
        // Container events - identical processing
        crate::ujson::Event::ObjectStart => Some(EventResult::Complete(Event::StartObject)),
        crate::ujson::Event::ObjectEnd => Some(EventResult::Complete(Event::EndObject)),
        crate::ujson::Event::ArrayStart => Some(EventResult::Complete(Event::StartArray)),
        crate::ujson::Event::ArrayEnd => Some(EventResult::Complete(Event::EndArray)),

        // Primitive values - identical processing
        crate::ujson::Event::End(EventToken::True) => {
            Some(EventResult::Complete(Event::Bool(true)))
        }
        crate::ujson::Event::End(EventToken::False) => {
            Some(EventResult::Complete(Event::Bool(false)))
        }
        crate::ujson::Event::End(EventToken::Null) => Some(EventResult::Complete(Event::Null)),

        // Content extraction triggers - identical logic
        crate::ujson::Event::End(EventToken::String) => Some(EventResult::ExtractString),
        crate::ujson::Event::End(EventToken::Key) => Some(EventResult::ExtractKey),
        crate::ujson::Event::End(EventToken::Number) => Some(EventResult::ExtractNumber(false)),
        crate::ujson::Event::End(EventToken::NumberAndArray) => {
            Some(EventResult::ExtractNumber(true))
        }
        crate::ujson::Event::End(EventToken::NumberAndObject) => {
            Some(EventResult::ExtractNumber(true))
        }

        // All other events need parser-specific handling
        _ => None,
    }
}

/// Process a specific byte through the tokenizer (for cases where byte is already available)
pub fn process_byte_through_tokenizer<T: crate::ujson::BitBucket, C: crate::ujson::DepthCounter>(
    byte: u8,
    tokenizer: &mut crate::ujson::Tokenizer<T, C>,
    event_storage: &mut [Option<crate::ujson::Event>; 2],
) -> Result<(), ParseError> {
    clear_events(event_storage);
    let mut callback = create_tokenizer_callback(event_storage);
    tokenizer
        .parse_chunk(&[byte], &mut callback)
        .map_err(ParseError::TokenizerError)?;
    Ok(())
}

/// Finish the tokenizer and collect any final events
pub fn finish_tokenizer<T: crate::ujson::BitBucket, C: crate::ujson::DepthCounter>(
    tokenizer: &mut crate::ujson::Tokenizer<T, C>,
    event_storage: &mut [Option<crate::ujson::Event>; 2],
) -> Result<(), ParseError> {
    clear_events(event_storage);
    let mut callback = create_tokenizer_callback(event_storage);
    tokenizer
        .finish(&mut callback)
        .map_err(ParseError::TokenizerError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_events() {
        assert!(matches!(
            process_simple_events(&crate::ujson::Event::ObjectStart),
            Some(EventResult::Complete(Event::StartObject))
        ));

        assert!(matches!(
            process_simple_events(&crate::ujson::Event::ArrayEnd),
            Some(EventResult::Complete(Event::EndArray))
        ));
    }

    #[test]
    fn test_primitive_events() {
        assert!(matches!(
            process_simple_events(&crate::ujson::Event::End(EventToken::True)),
            Some(EventResult::Complete(Event::Bool(true)))
        ));

        assert!(matches!(
            process_simple_events(&crate::ujson::Event::End(EventToken::Null)),
            Some(EventResult::Complete(Event::Null))
        ));
    }

    #[test]
    fn test_extraction_triggers() {
        assert!(matches!(
            process_simple_events(&crate::ujson::Event::End(EventToken::String)),
            Some(EventResult::ExtractString)
        ));

        assert!(matches!(
            process_simple_events(&crate::ujson::Event::End(EventToken::Number)),
            Some(EventResult::ExtractNumber(false))
        ));

        assert!(matches!(
            process_simple_events(&crate::ujson::Event::End(EventToken::NumberAndArray)),
            Some(EventResult::ExtractNumber(true))
        ));
    }

    #[test]
    fn test_complex_events_not_handled() {
        assert!(process_simple_events(&crate::ujson::Event::Begin(EventToken::String)).is_none());
        assert!(
            process_simple_events(&crate::ujson::Event::Begin(EventToken::EscapeQuote)).is_none()
        );
    }

    // Mock ContentExtractor for testing
    struct MockContentExtractor {
        position: usize,
        state: crate::shared::State,
        string_begin_calls: Vec<usize>,
    }

    impl MockContentExtractor {
        fn new() -> Self {
            Self {
                position: 42,
                state: crate::shared::State::None,
                string_begin_calls: Vec::new(),
            }
        }
    }

    impl EscapeHandler for MockContentExtractor {
        fn parser_state(&self) -> &crate::shared::State {
            &self.state
        }

        fn process_unicode_escape_with_collector(&mut self) -> Result<(), crate::ParseError> {
            Ok(())
        }

        fn handle_simple_escape_char(&mut self, _escape_char: u8) -> Result<(), crate::ParseError> {
            Ok(())
        }
    }

    impl ContentExtractor for MockContentExtractor {
        fn current_position(&self) -> usize {
            self.position
        }

        fn begin_string_content(&mut self, pos: usize) {
            self.string_begin_calls.push(pos);
        }

        fn parser_state_mut(&mut self) -> &mut crate::shared::State {
            &mut self.state
        }

        fn unicode_escape_collector_mut(
            &mut self,
        ) -> &mut crate::escape_processor::UnicodeEscapeCollector {
            unimplemented!("Mock doesn't need unicode collector")
        }

        fn extract_string_content(
            &mut self,
            _start_pos: usize,
        ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn extract_key_content(
            &mut self,
            _start_pos: usize,
        ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn extract_number_content(
            &mut self,
            _start_pos: usize,
            _from_container_end: bool,
        ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }

        fn extract_number(
            &mut self,
            _start_pos: usize,
            _from_container_end: bool,
            _finished: bool,
        ) -> Result<crate::Event<'_, '_>, crate::ParseError> {
            unimplemented!("Mock doesn't need extraction")
        }
    }

    #[test]
    fn test_begin_events_key() {
        let mut context = MockContentExtractor::new();
        let event = crate::ujson::Event::Begin(EventToken::Key);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(context.state, crate::shared::State::Key(42)));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_string() {
        let mut context = MockContentExtractor::new();
        let event = crate::ujson::Event::Begin(EventToken::String);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(context.state, crate::shared::State::String(42)));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_number() {
        let mut context = MockContentExtractor::new();
        let event = crate::ujson::Event::Begin(EventToken::Number);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        // Number should get position adjusted by ContentRange::number_start_from_current
        assert!(matches!(context.state, crate::shared::State::Number(_)));
        assert_eq!(context.string_begin_calls, Vec::<usize>::new()); // No string calls for numbers
    }

    #[test]
    fn test_begin_events_primitives() {
        let mut context = MockContentExtractor::new();

        for token in [EventToken::True, EventToken::False, EventToken::Null] {
            let event = crate::ujson::Event::Begin(token);
            let result = process_begin_events(&event, &mut context);
            assert!(matches!(result, Some(EventResult::Continue)));
        }

        // Should not affect state or string processing
        assert!(matches!(context.state, crate::shared::State::None));
        assert!(context.string_begin_calls.is_empty());
    }

    #[test]
    fn test_begin_events_not_handled() {
        let mut context = MockContentExtractor::new();
        let event = crate::ujson::Event::Begin(EventToken::EscapeQuote);

        let result = process_begin_events(&event, &mut context);

        assert!(result.is_none());
        assert!(matches!(context.state, crate::shared::State::None));
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
            callback(crate::ujson::Event::ObjectStart, 1);
        }
        assert!(have_events(&event_storage));
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_none());

        {
            let mut callback = create_tokenizer_callback(&mut event_storage);
            // Add second event
            callback(crate::ujson::Event::ArrayStart, 1);
        }
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_some());

        {
            let mut callback = create_tokenizer_callback(&mut event_storage);
            // Storage is full, third event should be ignored (no panic)
            callback(crate::ujson::Event::ObjectEnd, 1);
        }
        assert!(event_storage[0].is_some());
        assert!(event_storage[1].is_some());
    }

    #[test]
    fn test_event_extraction() {
        let mut event_storage = [
            Some(crate::ujson::Event::ObjectStart),
            Some(crate::ujson::Event::ArrayStart),
        ];

        // Extract first event
        let first = take_first_event(&mut event_storage);
        assert!(matches!(first, Some(crate::ujson::Event::ObjectStart)));
        assert!(event_storage[0].is_none());
        assert!(event_storage[1].is_some());

        // Extract second event
        let second = take_first_event(&mut event_storage);
        assert!(matches!(second, Some(crate::ujson::Event::ArrayStart)));
        assert!(event_storage[0].is_none());
        assert!(event_storage[1].is_none());

        // No more events
        let none = take_first_event(&mut event_storage);
        assert!(none.is_none());
        assert!(!have_events(&event_storage));
    }
}
