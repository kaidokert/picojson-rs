// SPDX-License-Identifier: Apache-2.0

//! Shared event processing logic between SliceParser and StreamParser.
//!
//! This module extracts the common event handling patterns to reduce code duplication
//! while preserving the performance characteristics of each parser type.

use crate::ujson::EventToken;
use crate::{Event, ParseError};

/// Parser context trait for abstracting common operations
pub trait ParserContext {
    /// Get current position in the input
    fn current_position(&self) -> usize;

    /// Begin string/key content processing at current position
    fn begin_string_content(&mut self, pos: usize);

    /// Set parser state
    fn set_parser_state(&mut self, state: crate::shared::State);
}

/// Escape handling trait for abstracting escape sequence processing between parsers
pub trait EscapeHandler {
    /// Get the current parser state for escape context checking
    fn parser_state(&self) -> &crate::shared::State;

    /// Reset all unicode escape collector state (including pending surrogates)
    fn reset_unicode_collector_all(&mut self);

    /// Reset unicode escape collector for new sequence (preserving pending surrogates)
    fn reset_unicode_collector(&mut self);

    /// Check if there's a pending high surrogate waiting for low surrogate
    fn has_pending_high_surrogate(&self) -> bool;

    /// Process Unicode escape sequence using shared collector logic
    fn process_unicode_escape_with_collector(&mut self) -> Result<(), crate::ParseError>;

    /// Handle a simple escape character (after EscapeProcessor conversion)
    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), crate::ParseError>;
}

/// Minimal buffer interface for position tracking and content extraction
pub trait BufferLike {
    /// Current position in the input stream/slice
    fn current_position(&self) -> usize;

    /// Extract a slice of bytes from start to end position
    /// Used for number and string content extraction
    fn get_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError>;
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
pub fn process_begin_events<C: ParserContext>(
    event: &crate::ujson::Event,
    context: &mut C,
) -> Option<EventResult<'static, 'static>> {
    use crate::shared::{ContentRange, State};

    match event {
        // String/Key Begin events - nearly identical patterns
        crate::ujson::Event::Begin(EventToken::Key) => {
            let pos = context.current_position();
            context.set_parser_state(State::Key(pos));
            context.begin_string_content(pos);
            Some(EventResult::Continue)
        }
        crate::ujson::Event::Begin(EventToken::String) => {
            let pos = context.current_position();
            context.set_parser_state(State::String(pos));
            context.begin_string_content(pos);
            Some(EventResult::Continue)
        }

        // Number Begin events - identical logic
        crate::ujson::Event::Begin(
            EventToken::Number | EventToken::NumberAndArray | EventToken::NumberAndObject,
        ) => {
            let pos = context.current_position();
            let number_start = ContentRange::number_start_from_current(pos);
            context.set_parser_state(State::Number(number_start));
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
pub trait ContentExtractor: EscapeHandler {
    /// Get mutable access to parser state
    fn parser_state_mut(&mut self) -> &mut crate::shared::State;

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

    /// Shared validation and extraction for string content
    fn validate_and_extract_string(&mut self) -> Result<crate::Event<'_, '_>, crate::ParseError> {
        let start_pos = match *self.parser_state() {
            crate::shared::State::String(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        // Check for incomplete surrogate pairs before ending the string
        if self.has_pending_high_surrogate() {
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
        if self.has_pending_high_surrogate() {
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
pub fn create_tokenizer_callback<'a>(
    event_storage: &'a mut [Option<crate::ujson::Event>; 2],
) -> impl FnMut(crate::ujson::Event, usize) + 'a {
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

/// Process simple escape sequence events that have similar patterns between parsers
pub fn process_simple_escape_event<E: EscapeHandler>(
    escape_token: &EventToken,
    escape_handler: &mut E,
) -> Result<(), crate::ParseError> {
    // Clear any pending high surrogate state when we encounter a simple escape
    // This ensures that interrupted surrogate pairs (like \uD801\n\uDC37) are properly rejected
    escape_handler.reset_unicode_collector_all();

    // Use unified escape token processing from EscapeProcessor
    let unescaped_char =
        crate::escape_processor::EscapeProcessor::process_escape_token(escape_token)?;

    // Only process if we're inside a string or key
    match escape_handler.parser_state() {
        crate::shared::State::String(_) | crate::shared::State::Key(_) => {
            escape_handler.handle_simple_escape_char(unescaped_char)?;
        }
        _ => {} // Ignore if not in string/key context
    }

    Ok(())
}

/// Process Unicode escape begin/end events that have similar patterns between parsers
pub fn process_unicode_escape_events<E: EscapeHandler>(
    event: &crate::ujson::Event,
    escape_handler: &mut E,
) -> Result<bool, crate::ParseError> {
    match event {
        crate::ujson::Event::Begin(EventToken::UnicodeEscape) => {
            // Start Unicode escape collection - reset collector for new sequence
            // Only handle if we're inside a string or key
            match escape_handler.parser_state() {
                crate::shared::State::String(_) | crate::shared::State::Key(_) => {
                    escape_handler.reset_unicode_collector();
                }
                _ => {} // Ignore if not in string/key context
            }
            Ok(true) // Event was handled
        }
        crate::ujson::Event::End(EventToken::UnicodeEscape) => {
            // Handle end of Unicode escape sequence (\uXXXX)
            match escape_handler.parser_state() {
                crate::shared::State::String(_) | crate::shared::State::Key(_) => {
                    escape_handler.process_unicode_escape_with_collector()?;
                }
                _ => {} // Ignore if not in string/key context
            }
            Ok(true) // Event was handled
        }
        _ => Ok(false), // Event was not handled
    }
}

/// Process simple container and primitive events that are identical between parsers
pub fn process_simple_events(event: crate::ujson::Event) -> Option<EventResult<'static, 'static>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_events() {
        assert!(matches!(
            process_simple_events(crate::ujson::Event::ObjectStart),
            Some(EventResult::Complete(Event::StartObject))
        ));

        assert!(matches!(
            process_simple_events(crate::ujson::Event::ArrayEnd),
            Some(EventResult::Complete(Event::EndArray))
        ));
    }

    #[test]
    fn test_primitive_events() {
        assert!(matches!(
            process_simple_events(crate::ujson::Event::End(EventToken::True)),
            Some(EventResult::Complete(Event::Bool(true)))
        ));

        assert!(matches!(
            process_simple_events(crate::ujson::Event::End(EventToken::Null)),
            Some(EventResult::Complete(Event::Null))
        ));
    }

    #[test]
    fn test_extraction_triggers() {
        assert!(matches!(
            process_simple_events(crate::ujson::Event::End(EventToken::String)),
            Some(EventResult::ExtractString)
        ));

        assert!(matches!(
            process_simple_events(crate::ujson::Event::End(EventToken::Number)),
            Some(EventResult::ExtractNumber(false))
        ));

        assert!(matches!(
            process_simple_events(crate::ujson::Event::End(EventToken::NumberAndArray)),
            Some(EventResult::ExtractNumber(true))
        ));
    }

    #[test]
    fn test_complex_events_not_handled() {
        assert!(process_simple_events(crate::ujson::Event::Begin(EventToken::String)).is_none());
        assert!(
            process_simple_events(crate::ujson::Event::Begin(EventToken::EscapeQuote)).is_none()
        );
    }

    // Mock ParserContext for testing
    struct MockParserContext {
        position: usize,
        state: Option<crate::shared::State>,
        string_begin_calls: Vec<usize>,
    }

    impl MockParserContext {
        fn new() -> Self {
            Self {
                position: 42,
                state: None,
                string_begin_calls: Vec::new(),
            }
        }
    }

    impl ParserContext for MockParserContext {
        fn current_position(&self) -> usize {
            self.position
        }

        fn begin_string_content(&mut self, pos: usize) {
            self.string_begin_calls.push(pos);
        }

        fn set_parser_state(&mut self, state: crate::shared::State) {
            self.state = Some(state);
        }
    }

    #[test]
    fn test_begin_events_key() {
        let mut context = MockParserContext::new();
        let event = crate::ujson::Event::Begin(EventToken::Key);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(context.state, Some(crate::shared::State::Key(42))));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_string() {
        let mut context = MockParserContext::new();
        let event = crate::ujson::Event::Begin(EventToken::String);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        assert!(matches!(
            context.state,
            Some(crate::shared::State::String(42))
        ));
        assert_eq!(context.string_begin_calls, vec![42]);
    }

    #[test]
    fn test_begin_events_number() {
        let mut context = MockParserContext::new();
        let event = crate::ujson::Event::Begin(EventToken::Number);

        let result = process_begin_events(&event, &mut context);

        assert!(matches!(result, Some(EventResult::Continue)));
        // Number should get position adjusted by ContentRange::number_start_from_current
        assert!(matches!(
            context.state,
            Some(crate::shared::State::Number(_))
        ));
        assert_eq!(context.string_begin_calls, Vec::<usize>::new()); // No string calls for numbers
    }

    #[test]
    fn test_begin_events_primitives() {
        let mut context = MockParserContext::new();

        for token in [EventToken::True, EventToken::False, EventToken::Null] {
            let event = crate::ujson::Event::Begin(token);
            let result = process_begin_events(&event, &mut context);
            assert!(matches!(result, Some(EventResult::Continue)));
        }

        // Should not affect state or string processing
        assert!(context.state.is_none());
        assert!(context.string_begin_calls.is_empty());
    }

    #[test]
    fn test_begin_events_not_handled() {
        let mut context = MockParserContext::new();
        let event = crate::ujson::Event::Begin(EventToken::EscapeQuote);

        let result = process_begin_events(&event, &mut context);

        assert!(result.is_none());
        assert!(context.state.is_none());
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
