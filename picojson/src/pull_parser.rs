// SPDX-License-Identifier: Apache-2.0

use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::{EscapeProcessor, UnicodeEscapeCollector};
use crate::shared::{ContentRange, Event, ParseError, ParserErrorHandler, ParserState, State};
use crate::slice_input_buffer::{InputBuffer, SliceInputBuffer};
use crate::ujson;
use ujson::{EventToken, Tokenizer};

use ujson::{BitStackConfig, DefaultConfig};

/// Result of processing a tokenizer event
enum EventResult<'a, 'b> {
    /// Event processing complete, return this event
    Complete(Event<'a, 'b>),
    /// Continue processing, no event to return yet
    Continue,
    /// Extract string content from current state
    ExtractString,
    /// Extract key content from current state
    ExtractKey,
    /// Extract number content from current state,
    /// bool indicates if number was terminated by container delimiter
    ExtractNumber(bool),
}

/// A flexible pull parser for JSON that yields events on demand.
/// Generic over BitStack storage type for configurable nesting depth.
// Lifetime 'a is the input buffer lifetime
// lifetime 'b is the scratch/copy buffer lifetime
pub struct PullParser<'a, 'b, C: BitStackConfig = DefaultConfig> {
    tokenizer: Tokenizer<C::Bucket, C::Counter>,
    buffer: SliceInputBuffer<'a>,
    parser_state: ParserState,
    copy_on_escape: CopyOnEscape<'a, 'b>,
    /// Shared Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
}

/// Methods for the pull parser.
impl<'a> PullParser<'a, '_, DefaultConfig> {
    /// Creates a new parser for the given JSON input.
    ///
    /// This parser assumes no string escapes will be encountered. If escapes are found,
    /// parsing will fail with `ScratchBufferFull` error.
    ///
    /// For JSON with potential string escapes, use `with_buffer()` instead.
    ///
    /// # Arguments
    /// * `input` - A string slice containing the JSON data to be parsed.
    ///
    /// # Example
    /// ```
    /// use picojson::PullParser;
    /// let parser = PullParser::new(r#"{"name": "value"}"#);
    /// ```
    pub fn new(input: &'a str) -> Self {
        Self::with_config(input)
    }
}

/// Constructor with scratch buffer for PullParser using DefaultConfig
impl<'a, 'b> PullParser<'a, 'b, DefaultConfig> {
    /// Creates a new parser for the given JSON input with external scratch buffer.
    ///
    /// Use this when your JSON contains string escapes (like `\n`, `\"`, `\u0041`) that
    /// need to be unescaped during parsing.
    ///
    /// # Arguments
    /// * `input` - A string slice containing the JSON data to be parsed.
    /// * `scratch_buffer` - A mutable byte slice for temporary string unescaping operations.
    ///                      This buffer needs to be at least as long as the longest
    ///                      contiguous token (string, key, number) in the input.
    ///
    /// # Example
    /// ```
    /// use picojson::PullParser;
    /// let mut scratch = [0u8; 1024];
    /// let parser = PullParser::with_buffer(r#"{"msg": "Hello\nWorld"}"#, &mut scratch);
    /// ```
    pub fn with_buffer(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        Self::with_config_and_buffer(input, scratch_buffer)
    }
}

/// Generic constructor for PullParser with custom configurations
impl<'a, 'b, C: BitStackConfig> PullParser<'a, 'b, C> {
    /// Creates a new parser with a custom `BitStackConfig`.
    ///
    /// This parser assumes no string escapes will be encountered. If escapes are found,
    /// parsing will fail. For JSON with escapes, use `with_config_and_buffer`.
    pub fn with_config(input: &'a str) -> Self {
        Self::with_config_and_buffer(input, &mut [])
    }

    /// Creates a new parser with a custom `BitStackConfig` and a user-provided scratch buffer.
    ///
    /// Use this when your JSON contains string escapes (like `\n`, `\"`, `\u0041`).
    ///
    /// # Arguments
    /// * `input` - A string slice containing the JSON data to be parsed.
    /// * `scratch_buffer` - A mutable byte slice for temporary string unescaping operations.
    ///                      This buffer needs to be at least as long as the longest
    ///                      contiguous token (string, key, number) in the input.
    pub fn with_config_and_buffer(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        let data = input.as_bytes();
        let copy_on_escape = CopyOnEscape::new(data, scratch_buffer);
        PullParser {
            tokenizer: Tokenizer::new(),
            buffer: SliceInputBuffer::new(data),
            parser_state: ParserState::new(),
            copy_on_escape,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
        }
    }

    fn have_events(&self) -> bool {
        self.parser_state.evts.iter().any(|evt| evt.is_some())
    }

    /// Helper function to parse a number from the buffer given a start position.
    /// Uses unified number parsing logic with centralized delimiter handling.
    fn parse_number_from_buffer(
        &mut self,
        start: usize,
        from_container_end: bool,
    ) -> Result<Event, ParseError> {
        crate::number_parser::parse_number_event(&self.buffer, start, from_container_end)
    }

    /// Helper method to handle simple escape tokens using EscapeProcessor
    /// Converts EventToken back to original escape character and processes it
    fn handle_simple_escape_token(
        &mut self,
        escape_token: &EventToken,
    ) -> Result<Option<Event>, ParseError> {
        // Use unified escape token processing
        let unescaped_char = EscapeProcessor::process_escape_token(escape_token)?;

        // Handle the escape using existing logic
        self.handle_escape_event(unescaped_char)
    }

    /// Handles escape sequence events by delegating to CopyOnEscape if we're inside a string or key
    fn handle_escape_event(&mut self, escape_char: u8) -> Result<Option<Event>, ParseError> {
        if let State::String(_) | State::Key(_) = self.parser_state.state {
            self.copy_on_escape
                .handle_escape(self.buffer.current_pos(), escape_char)?;
        }
        Ok(None)
    }

    /// Process Unicode escape sequence using shared UnicodeEscapeCollector
    /// Extracts hex digits from buffer and processes them through the collector
    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        let current_pos = self.buffer.current_pos();
        let hex_slice_provider = |start, end| self.buffer.slice(start, end).map_err(Into::into);

        let mut utf8_buf = [0u8; 4];
        let (utf8_bytes, escape_start_pos) =
            crate::escape_processor::process_unicode_escape_sequence(
                current_pos,
                &mut self.unicode_escape_collector,
                hex_slice_provider,
                &mut utf8_buf,
            )?;

        self.copy_on_escape
            .handle_unicode_escape(escape_start_pos, utf8_bytes)
    }

    fn pull_tokenizer_events(&mut self) -> Result<(), ParseError> {
        use crate::slice_input_buffer::InputBuffer;
        if self.buffer.is_past_end() {
            return Err(ParseError::EndOfData);
        }
        let mut callback = |event, _len| {
            for evt in self.parser_state.evts.iter_mut() {
                if evt.is_none() {
                    *evt = Some(event);
                    return;
                }
            }
        };

        let res = match self.buffer.consume_byte() {
            Err(crate::slice_input_buffer::Error::ReachedEnd) => {
                self.tokenizer.finish(&mut callback)
            }
            Err(crate::slice_input_buffer::Error::InvalidSliceBounds) => {
                return Err(ParseError::UnexpectedState(
                    "Invalid slice bounds in consume_byte",
                ));
            }
            Ok(byte) => self.tokenizer.parse_chunk(&[byte], &mut callback),
        };

        if let Err(_tokenizer_error) = res {
            return Err(ParseError::TokenizerError);
        }
        Ok(())
    }

    pub fn next(&mut self) -> Option<Result<Event, ParseError>> {
        match self.next_event() {
            Ok(Event::EndDocument) => None,
            other => Some(other),
        }
    }

    /// Returns the next JSON event or an error if parsing fails.
    /// Parsing continues until `EndDocument` is returned or an error occurs.
    pub fn next_event(&mut self) -> Result<Event, ParseError> {
        if self.buffer.is_past_end() {
            return Ok(Event::EndDocument);
        }
        loop {
            while !self.have_events() {
                self.pull_tokenizer_events()?;
                if self.buffer.is_past_end() {
                    return Ok(Event::EndDocument);
                }
            }
            // Find and move out the first available event to avoid holding mutable borrow during processing
            let taken_event = {
                let mut found_event = None;
                for evt in self.parser_state.evts.iter_mut() {
                    if evt.is_some() {
                        found_event = evt.take();
                        break;
                    }
                }
                found_event
            };

            if let Some(taken) = taken_event {
                let res = match taken {
                    // Container events
                    ujson::Event::ObjectStart => EventResult::Complete(Event::StartObject),
                    ujson::Event::ObjectEnd => EventResult::Complete(Event::EndObject),
                    ujson::Event::ArrayStart => EventResult::Complete(Event::StartArray),
                    ujson::Event::ArrayEnd => EventResult::Complete(Event::EndArray),

                    // String/Key events
                    ujson::Event::Begin(EventToken::Key) => {
                        self.parser_state.state = State::Key(self.buffer.current_pos());
                        self.copy_on_escape.begin_string(self.buffer.current_pos());
                        EventResult::Continue
                    }
                    ujson::Event::End(EventToken::Key) => EventResult::ExtractKey,
                    ujson::Event::Begin(EventToken::String) => {
                        self.parser_state.state = State::String(self.buffer.current_pos());
                        self.copy_on_escape.begin_string(self.buffer.current_pos());
                        EventResult::Continue
                    }
                    ujson::Event::End(EventToken::String) => EventResult::ExtractString,

                    // Number events
                    ujson::Event::Begin(
                        EventToken::Number
                        | EventToken::NumberAndArray
                        | EventToken::NumberAndObject,
                    ) => {
                        let number_start =
                            ContentRange::number_start_from_current(self.buffer.current_pos());
                        self.parser_state.state = State::Number(number_start);
                        EventResult::Continue
                    }
                    ujson::Event::End(EventToken::Number) => EventResult::ExtractNumber(false),
                    ujson::Event::End(EventToken::NumberAndArray) => {
                        EventResult::ExtractNumber(true)
                    }
                    ujson::Event::End(EventToken::NumberAndObject) => {
                        EventResult::ExtractNumber(true)
                    }
                    // Boolean and null values
                    ujson::Event::Begin(
                        EventToken::True | EventToken::False | EventToken::Null,
                    ) => EventResult::Continue,
                    ujson::Event::End(EventToken::True) => EventResult::Complete(Event::Bool(true)),
                    ujson::Event::End(EventToken::False) => {
                        EventResult::Complete(Event::Bool(false))
                    }
                    ujson::Event::End(EventToken::Null) => EventResult::Complete(Event::Null),
                    // Escape sequence handling
                    ujson::Event::Begin(
                        escape_token @ (EventToken::EscapeQuote
                        | EventToken::EscapeBackslash
                        | EventToken::EscapeSlash
                        | EventToken::EscapeBackspace
                        | EventToken::EscapeFormFeed
                        | EventToken::EscapeNewline
                        | EventToken::EscapeCarriageReturn
                        | EventToken::EscapeTab),
                    ) => {
                        // Use EscapeProcessor for all simple escape sequences
                        self.handle_simple_escape_token(&escape_token)?;
                        EventResult::Continue
                    }
                    ujson::Event::Begin(EventToken::UnicodeEscape) => {
                        // Start Unicode escape collection - reset collector for new sequence
                        // Only handle if we're inside a string or key
                        match self.parser_state.state {
                            State::String(_) | State::Key(_) => {
                                self.unicode_escape_collector.reset();
                            }
                            _ => {} // Ignore if not in string/key
                        }
                        EventResult::Continue
                    }
                    ujson::Event::End(EventToken::UnicodeEscape) => {
                        // Handle end of Unicode escape sequence (\uXXXX) using shared collector
                        match self.parser_state.state {
                            State::String(_) | State::Key(_) => {
                                // Process Unicode escape using shared collector logic
                                self.process_unicode_escape_with_collector()?;
                            }
                            _ => {} // Ignore if not in string/key context
                        }
                        EventResult::Continue
                    }
                    // EscapeSequence events (only emitted when flag is enabled, ignored in original parser)
                    ujson::Event::Begin(EventToken::EscapeSequence) => {
                        // Ignore in original parser since it uses slice-based parsing
                        EventResult::Continue
                    }
                    ujson::Event::End(EventToken::EscapeSequence) => {
                        // Ignore in original parser since it uses slice-based parsing
                        EventResult::Continue
                    }
                    ujson::Event::End(
                        EventToken::EscapeQuote
                        | EventToken::EscapeBackslash
                        | EventToken::EscapeSlash
                        | EventToken::EscapeBackspace
                        | EventToken::EscapeFormFeed
                        | EventToken::EscapeNewline
                        | EventToken::EscapeCarriageReturn
                        | EventToken::EscapeTab,
                    ) => {
                        // End of escape sequence - ignored here
                        EventResult::Continue
                    }
                    #[cfg(test)]
                    ujson::Event::Uninitialized => {
                        return Err(ParseError::UnexpectedState("Uninitialized event"));
                    }
                };
                match res {
                    EventResult::Complete(event) => break Ok(event),
                    EventResult::Continue => continue,
                    EventResult::ExtractKey => {
                        if let State::Key(_start) = self.parser_state.state {
                            self.parser_state.state = State::None;
                            // Use CopyOnEscape to get the final key result
                            let end_pos = ContentRange::end_position_excluding_delimiter(
                                self.buffer.current_pos(),
                            );
                            let key_result = self.copy_on_escape.end_string(end_pos)?;
                            break Ok(Event::Key(key_result));
                        } else {
                            break Err(ParserErrorHandler::state_mismatch("key", "end"));
                        }
                    }
                    EventResult::ExtractString => {
                        if let State::String(_value) = self.parser_state.state {
                            self.parser_state.state = State::None;
                            // Use CopyOnEscape to get the final string result
                            let end_pos = ContentRange::end_position_excluding_delimiter(
                                self.buffer.current_pos(),
                            );
                            let value_result = self.copy_on_escape.end_string(end_pos)?;
                            break Ok(Event::String(value_result));
                        } else {
                            break Err(ParserErrorHandler::state_mismatch("string", "end"));
                        }
                    }
                    EventResult::ExtractNumber(from_container_end) => {
                        if let State::Number(start) = self.parser_state.state {
                            // Reset state before parsing to stop selective copying
                            self.parser_state.state = State::None;
                            let event = self.parse_number_from_buffer(start, from_container_end)?;
                            break Ok(event);
                        } else {
                            break Err(ParseError::UnexpectedState(
                                "Number end without Number start",
                            ));
                        }
                    }
                }
            } else {
                // No event available - this shouldn't happen since we ensured have_events() above
                break Err(ParseError::UnexpectedState(
                    "No events available after ensuring events exist".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::String;
    use test_log::test;

    #[test]
    fn make_parser() {
        let input = r#"{"key": "value"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);
        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
        assert_eq!(
            parser.next_event(),
            Ok(Event::String(String::Borrowed("value")))
        );
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn parse_number() {
        let input = r#"{"key": 1242}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);
        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
        // Check number value using new JsonNumber API
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                assert_eq!(num.as_str(), "1242");
                assert_eq!(num.as_int(), Some(1242));
            }
            other => panic!("Expected Number, got: {:?}", other),
        }
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn parse_bool_and_null() {
        let input = r#"{"key": true, "key2": false, "key3": null}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);
        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
        assert_eq!(parser.next_event(), Ok(Event::Bool(true)));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("key2")))
        );
        assert_eq!(parser.next_event(), Ok(Event::Bool(false)));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("key3")))
        );
        assert_eq!(parser.next_event(), Ok(Event::Null));
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn parse_array() {
        #[cfg(feature = "float-error")]
        let input = r#"{"key": [1, 2, 3]}"#; // No floats for float-error config
        #[cfg(not(feature = "float-error"))]
        let input = r#"{"key": [1, 2.2, 3]}"#; // Include float for other configs

        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);
        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
        assert_eq!(parser.next_event(), Ok(Event::StartArray));

        // First number: 1 (integer)
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                assert_eq!(num.as_str(), "1");
                assert_eq!(num.as_int(), Some(1));
            }
            other => panic!("Expected Number(1), got: {:?}", other),
        }

        // Second number: depends on configuration
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                #[cfg(feature = "float-error")]
                {
                    assert_eq!(num.as_str(), "2");
                    assert_eq!(num.as_int(), Some(2));
                }
                #[cfg(not(feature = "float-error"))]
                {
                    assert_eq!(num.as_str(), "2.2");
                    #[cfg(feature = "float")]
                    assert_eq!(num.as_f64(), Some(2.2));
                    #[cfg(not(feature = "float-error"))]
                    assert!(num.is_float());
                }
            }
            other => panic!("Expected Number, got: {:?}", other),
        }

        // Third number: 3 (integer)
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                assert_eq!(num.as_str(), "3");
                assert_eq!(num.as_int(), Some(3));
            }
            other => panic!("Expected Number(3), got: {:?}", other),
        }

        assert_eq!(parser.next_event(), Ok(Event::EndArray));
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_simple_parser_api() {
        let input = r#"{"name": "test"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("name")))
        );
        assert_eq!(
            parser.next_event(),
            Ok(Event::String(String::Borrowed("test")))
        );
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_parser_with_escaped_strings() {
        // Use regular string literal to properly include escape sequences
        let input = "{\"name\": \"John\\nDoe\", \"message\": \"Hello\\tWorld!\"}";
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        // Test that the parser correctly handles escaped strings
        assert_eq!(parser.next_event(), Ok(Event::StartObject));

        // Key should be simple (no escapes) -> Borrowed
        if let Ok(Event::Key(key)) = parser.next_event() {
            assert_eq!(&*key, "name");
            // This should be the fast path (borrowed)
            assert!(matches!(key, String::Borrowed(_)));
        } else {
            panic!("Expected Key event");
        }

        // Value should have escapes -> Unescaped
        if let Ok(Event::String(value)) = parser.next_event() {
            assert_eq!(&*value, "John\nDoe");
            // This should be the slow path (unescaped)
            assert!(matches!(value, String::Unescaped(_)));
        } else {
            panic!("Expected String event");
        }

        // Second key should be simple -> Borrowed
        if let Ok(Event::Key(key)) = parser.next_event() {
            assert_eq!(&*key, "message");
            assert!(matches!(key, String::Borrowed(_)));
        } else {
            panic!("Expected Key event");
        }

        // Second value should have escapes -> Unescaped
        if let Ok(Event::String(value)) = parser.next_event() {
            assert_eq!(&*value, "Hello\tWorld!");
            assert!(matches!(value, String::Unescaped(_)));
        } else {
            panic!("Expected String event");
        }

        assert_eq!(parser.next_event(), Ok(Event::EndObject));
    }

    #[test]
    fn test_copy_on_escape_optimization() {
        // Use regular string literal to include proper escape sequences
        let input = "{\"simple\": \"no escapes\", \"complex\": \"has\\nescapes\"}";
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));

        // "simple" key should be borrowed (fast path)
        if let Ok(Event::Key(key)) = parser.next_event() {
            assert_eq!(&*key, "simple");
            assert!(matches!(key, String::Borrowed(_)));
        } else {
            panic!("Expected Key event");
        }

        // "no escapes" value should be borrowed (fast path)
        if let Ok(Event::String(value)) = parser.next_event() {
            assert_eq!(&*value, "no escapes");
            assert!(matches!(value, String::Borrowed(_)));
        } else {
            panic!("Expected String event");
        }

        // "complex" key should be borrowed (fast path)
        if let Ok(Event::Key(key)) = parser.next_event() {
            assert_eq!(&*key, "complex");
            assert!(matches!(key, String::Borrowed(_)));
        } else {
            panic!("Expected Key event");
        }

        // "has\\nescapes" value should be unescaped (slow path)
        if let Ok(Event::String(value)) = parser.next_event() {
            assert_eq!(&*value, "has\nescapes");
            assert!(matches!(value, String::Unescaped(_)));
        } else {
            panic!("Expected String event");
        }

        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_coe2_integration_multiple_escapes() {
        let input = r#"{"key": "a\nb\tc\rd"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));

        let string_event = parser.next_event().unwrap();
        match string_event {
            Event::String(String::Unescaped(s)) => {
                assert_eq!(s, "a\nb\tc\rd");
            }
            _ => panic!("Expected unescaped string value, got: {:?}", string_event),
        }
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_coe2_integration_zero_copy_path() {
        let input = r#"{"simple": "no_escapes_here"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("simple")))
        );

        // This should be borrowed (zero-copy) since no escapes
        let string_event = parser.next_event().unwrap();
        match string_event {
            Event::String(String::Borrowed(s)) => {
                assert_eq!(s, "no_escapes_here");
            }
            _ => panic!(
                "Expected borrowed string value for zero-copy, got: {:?}",
                string_event
            ),
        }
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_coe2_integration_mixed_strings() {
        let input = r#"["plain", "with\nescapes", "plain2", "more\tescapes"]"#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartArray));

        // First string: no escapes -> borrowed
        match parser.next_event().unwrap() {
            Event::String(String::Borrowed(s)) => assert_eq!(s, "plain"),
            other => panic!("Expected borrowed string, got: {:?}", other),
        }

        // Second string: has escapes -> unescaped
        match parser.next_event().unwrap() {
            Event::String(String::Unescaped(s)) => assert_eq!(s, "with\nescapes"),
            other => panic!("Expected unescaped string, got: {:?}", other),
        }

        // Third string: no escapes -> borrowed
        match parser.next_event().unwrap() {
            Event::String(String::Borrowed(s)) => assert_eq!(s, "plain2"),
            other => panic!("Expected borrowed string, got: {:?}", other),
        }

        // Fourth string: has escapes -> unescaped
        match parser.next_event().unwrap() {
            Event::String(String::Unescaped(s)) => assert_eq!(s, "more\tescapes"),
            other => panic!("Expected unescaped string, got: {:?}", other),
        }

        assert_eq!(parser.next_event(), Ok(Event::EndArray));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_unicode_escape_integration() {
        let input = r#"{"key": "Hello\u0041World"}"#; // \u0041 = 'A'
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));

        // The string with Unicode escape should be unescaped
        match parser.next_event().unwrap() {
            Event::String(String::Unescaped(s)) => {
                assert_eq!(s, "HelloAWorld");
            }
            other => panic!("Expected unescaped string value, got: {:?}", other),
        }

        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test_log::test]
    fn test_original_parser_escape_trace() {
        // Test escape sequence processing with logging
        let input = r#""a\nb""#;
        let mut scratch = [0u8; 1024];
        let mut parser = PullParser::with_buffer(input, &mut scratch);

        // Should get String with unescaped content
        let event = parser.next_event().unwrap();
        if let Event::String(s) = event {
            assert_eq!(&*s, "a\nb");
        } else {
            panic!("Expected String event, got {:?}", event);
        }

        // Should get EndDocument
        let event = parser.next_event().unwrap();
        assert_eq!(event, Event::EndDocument);
    }
}
