// SPDX-License-Identifier: Apache-2.0

use crate::copy_on_escape::CopyOnEscape;
use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::{
    finish_tokenizer, have_events, process_begin_escape_sequence_event, process_begin_events,
    process_byte_through_tokenizer, process_simple_escape_event, process_simple_events,
    process_unicode_escape_events, take_first_event, ContentExtractor, EscapeHandler, EventResult,
};
use crate::number_parser::NumberExtractor;
use crate::parse_error::ParseError;
use crate::shared::{
    ByteProvider, ContentRange, Event, ParserState, PullParser, State, UnexpectedState,
};
use crate::slice_input_buffer::{InputBuffer, SliceInputBuffer};
use crate::ujson;
use ujson::{EventToken, Tokenizer};

use ujson::{BitStackConfig, DefaultConfig};

/// A pull parser that parses JSON from a slice.
///
/// Generic over BitStack storage type for configurable nesting depth.
// Lifetime 'a is the input buffer lifetime
// lifetime 'b is the scratch/copy buffer lifetime
pub struct SliceParser<'a, 'b, C: BitStackConfig = DefaultConfig> {
    tokenizer: Tokenizer<C::Bucket, C::Counter>,
    buffer: SliceInputBuffer<'a>,
    parser_state: ParserState,
    copy_on_escape: CopyOnEscape<'a, 'b>,
    /// Shared Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
}

/// Methods for the pull parser.
impl<'a> SliceParser<'a, '_, DefaultConfig> {
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
    /// use picojson::SliceParser;
    /// let parser = SliceParser::new(r#"{"name": "value"}"#);
    /// ```
    pub fn new(input: &'a str) -> Self {
        Self::new_from_slice(input.as_bytes())
    }

    /// Creates a new parser from a byte slice.
    ///
    /// Assumes no string escapes will be encountered. For JSON with escapes, use [`with_buffer_from_slice`].
    ///
    /// # Example
    /// ```
    /// # use picojson::SliceParser;
    /// let parser = SliceParser::new_from_slice(br#"{"name": "value"}"#);
    /// ```
    ///
    /// [`with_buffer_from_slice`]: Self::with_buffer_from_slice
    pub fn new_from_slice(input: &'a [u8]) -> Self {
        Self::with_config_from_slice(input)
    }
}

/// Constructor with scratch buffer for SliceParser using DefaultConfig
impl<'a, 'b> SliceParser<'a, 'b, DefaultConfig> {
    /// Creates a new parser for the given JSON input with external scratch buffer.
    ///
    /// Use this when your JSON contains string escapes (like `\n`, `\"`, `\u0041`) that
    /// need to be unescaped during parsing.
    ///
    /// # Arguments
    /// * `input` - A string slice containing the JSON data to be parsed.
    /// * `scratch_buffer` - A mutable byte slice for temporary string unescaping operations.
    ///   This buffer needs to be at least as long as the longest
    ///   contiguous token (string, key, number) in the input.
    ///
    /// # Example
    /// ```
    /// use picojson::SliceParser;
    /// let mut scratch = [0u8; 1024];
    /// let parser = SliceParser::with_buffer(r#"{"msg": "Hello\nWorld"}"#, &mut scratch);
    /// ```
    pub fn with_buffer(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        Self::with_buffer_from_slice(input.as_bytes(), scratch_buffer)
    }

    /// Creates a new parser from a byte slice with a scratch buffer.
    ///
    /// Use when JSON contains string escapes that need unescaping.
    ///
    /// # Example
    /// ```
    /// # use picojson::SliceParser;
    /// let mut scratch = [0u8; 1024];
    /// let parser = SliceParser::with_buffer_from_slice(br#"{"msg": "Hello\nWorld"}"#, &mut scratch);
    /// ```
    pub fn with_buffer_from_slice(input: &'a [u8], scratch_buffer: &'b mut [u8]) -> Self {
        Self::with_config_and_buffer_from_slice(input, scratch_buffer)
    }
}

/// Generic constructor for SliceParser with custom configurations
impl<'a, 'b, C: BitStackConfig> SliceParser<'a, 'b, C> {
    /// Creates a new parser with a custom `BitStackConfig`.
    ///
    /// This parser assumes no string escapes will be encountered. If escapes are found,
    /// parsing will fail. For JSON with escapes, use `with_config_and_buffer`.
    pub fn with_config(input: &'a str) -> Self {
        Self::with_config_from_slice(input.as_bytes())
    }

    /// Creates a new parser from a byte slice with a custom `BitStackConfig`.
    ///
    /// Assumes no string escapes will be encountered. For JSON with escapes, use [`with_config_and_buffer_from_slice`].
    ///
    /// [`with_config_and_buffer_from_slice`]: Self::with_config_and_buffer_from_slice
    pub fn with_config_from_slice(input: &'a [u8]) -> Self {
        Self::with_config_and_buffer_from_slice(input, &mut [])
    }

    /// Creates a new parser with a custom `BitStackConfig` and a user-provided scratch buffer.
    ///
    /// Use this when your JSON contains string escapes (like `\n`, `\"`, `\u0041`).
    ///
    /// # Arguments
    /// * `input` - A string slice containing the JSON data to be parsed.
    /// * `scratch_buffer` - A mutable byte slice for temporary string unescaping operations.
    ///   This buffer needs to be at least as long as the longest
    ///   contiguous token (string, key, number) in the input.
    pub fn with_config_and_buffer(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        Self::with_config_and_buffer_from_slice(input.as_bytes(), scratch_buffer)
    }

    /// Creates a new parser from a byte slice with a custom `BitStackConfig` and scratch buffer.
    ///
    /// Use when JSON contains string escapes that need unescaping.
    /// This is the core constructor that all other constructors delegate to.
    pub fn with_config_and_buffer_from_slice(
        input: &'a [u8],
        scratch_buffer: &'b mut [u8],
    ) -> Self {
        let copy_on_escape = CopyOnEscape::new(input, scratch_buffer);
        SliceParser {
            tokenizer: Tokenizer::new(),
            buffer: SliceInputBuffer::new(input),
            parser_state: ParserState::new(),
            copy_on_escape,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
        }
    }

    /// Returns the next JSON event or an error if parsing fails.
    /// Parsing continues until `EndDocument` is returned or an error occurs.
    fn next_event_impl(&mut self) -> Result<Event<'_, '_>, ParseError> {
        if self.buffer.is_past_end() {
            return Ok(Event::EndDocument);
        }
        loop {
            while !have_events(&self.parser_state.evts) {
                if !self.pull_tokenizer_events()? {
                    return Ok(Event::EndDocument);
                }
            }
            // Find and move out the first available event to avoid holding mutable borrow during processing
            let taken_event = take_first_event(&mut self.parser_state.evts);

            if let Some(taken) = taken_event {
                // Process the event directly in the main loop using immediate processing pattern
                // First, try the shared simple event processor
                if let Some(simple_result) = process_simple_events(taken.clone()) {
                    match simple_result {
                        EventResult::Complete(event) => break Ok(event),
                        EventResult::ExtractString => break self.validate_and_extract_string(),
                        EventResult::ExtractKey => break self.validate_and_extract_key(),
                        EventResult::ExtractNumber(from_container_end) => {
                            break self.validate_and_extract_number(from_container_end)
                        }
                        EventResult::Continue => {
                            // Continue to next iteration for more events
                        }
                    }
                } else if let Some(begin_result) = process_begin_events(&taken, self) {
                    match begin_result {
                        EventResult::Complete(event) => break Ok(event),
                        EventResult::ExtractString => break self.validate_and_extract_string(),
                        EventResult::ExtractKey => break self.validate_and_extract_key(),
                        EventResult::ExtractNumber(from_container_end) => {
                            break self.validate_and_extract_number(from_container_end)
                        }
                        EventResult::Continue => {
                            // Continue to next iteration for more events
                        }
                    }
                } else {
                    match taken {
                        // Shared escape sequence lifecycle events (common pattern #1)
                        ujson::Event::Begin(EventToken::EscapeSequence) => {
                            // Use shared lifecycle interface for escape sequence start
                            process_begin_escape_sequence_event(self)?;
                            // Continue to next iteration
                        }
                        // Shared Unicode escape processing (common pattern #2)
                        _ if process_unicode_escape_events(&taken, self)? => {
                            // Unicode escape events handled by shared function
                            // Continue to next iteration
                        }
                        // Simple escape sequence handling - Begin events (SliceParser-specific)
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
                            // Use shared escape processing
                            process_simple_escape_event(&escape_token, self)?;
                            // Continue to next iteration
                        }
                        // Escape sequence lifecycle - End events (SliceParser-specific)
                        ujson::Event::End(EventToken::EscapeSequence) => {
                            // Ignore in original parser since it uses slice-based parsing
                            // Continue to next iteration
                        }
                        // All other events (container events, primitives, etc.) are handled by process_simple_events
                        _ => {
                            // Continue to next iteration
                        }
                    }
                }
            } else {
                // No event available - this shouldn't happen since we ensured have_events() above
                break Err(UnexpectedState::StateMismatch.into());
            }
        }
    }

    /// Pull events from tokenizer and return whether parsing should continue
    /// Returns false when past end (equivalent to self.buffer.is_past_end())
    fn pull_tokenizer_events(&mut self) -> Result<bool, ParseError> {
        if self.buffer.is_past_end() {
            return Ok(false); // Indicate end state instead of error
        }
        // Use ByteProvider implementation to get the next byte and process it
        if let Some(byte) = self.next_byte()? {
            process_byte_through_tokenizer(byte, &mut self.tokenizer, &mut self.parser_state.evts)?;
        } else {
            finish_tokenizer(&mut self.tokenizer, &mut self.parser_state.evts)?;
        }
        Ok(!self.buffer.is_past_end()) // Return continue state
    }
}

impl<'a, 'b, C: BitStackConfig> ContentExtractor for SliceParser<'a, 'b, C> {
    fn parser_state_mut(&mut self) -> &mut State {
        &mut self.parser_state.state
    }

    fn current_position(&self) -> usize {
        self.buffer.current_pos()
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.copy_on_escape.begin_string(pos);
    }

    fn unicode_escape_collector_mut(
        &mut self,
    ) -> &mut crate::escape_processor::UnicodeEscapeCollector {
        &mut self.unicode_escape_collector
    }

    fn extract_string_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // Use CopyOnEscape to get the final string result
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let value_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(Event::String(value_result))
    }

    fn extract_key_content(&mut self, _start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // Use CopyOnEscape to get the final key result
        let end_pos = ContentRange::end_position_excluding_delimiter(self.buffer.current_pos());
        let key_result = self.copy_on_escape.end_string(end_pos)?;
        Ok(Event::Key(key_result))
    }

    fn extract_number_content(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // Use shared number parsing with SliceParser-specific document end detection
        let at_document_end = self.buffer.is_empty();
        crate::number_parser::parse_number_with_delimiter_logic(
            &self.buffer,
            start_pos,
            from_container_end,
            at_document_end,
        )
    }
}

impl<'a, 'b, C: BitStackConfig> EscapeHandler for SliceParser<'a, 'b, C> {
    fn parser_state(&self) -> &State {
        &self.parser_state.state
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
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

        // Handle UTF-8 bytes if we have them (not a high surrogate waiting for low surrogate)
        if let Some(utf8_bytes) = utf8_bytes_opt {
            if had_pending_high_surrogate {
                // This is completing a surrogate pair - need to consume both escapes
                // First call: consume the high surrogate (6 bytes earlier)
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos, &[])?;
                // Second call: consume the low surrogate and write UTF-8
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos + 6, utf8_bytes)?;
            } else {
                // Single Unicode escape - normal processing
                self.copy_on_escape
                    .handle_unicode_escape(escape_start_pos, utf8_bytes)?;
            }
        }

        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.copy_on_escape
            .handle_escape(self.buffer.current_pos(), escape_char)?;
        Ok(())
    }

    /// Append a single literal byte - implement as single-byte range for consistency
    fn append_literal_byte(&mut self, _byte: u8) -> Result<(), ParseError> {
        // SliceParser doesn't typically need per-byte processing since it works with ranges
        // This could be implemented as a single-byte range if needed, but for now it's a no-op
        Ok(())
    }
}

impl<'a, 'b, C: BitStackConfig> PullParser for SliceParser<'a, 'b, C> {
    fn next_event(&mut self) -> Result<Event<'_, '_>, ParseError> {
        self.next_event_impl()
    }
}

impl<'a, 'b, C: BitStackConfig> crate::shared::ByteProvider for SliceParser<'a, 'b, C> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        use crate::slice_input_buffer::InputBuffer;
        match self.buffer.consume_byte() {
            Ok(byte) => Ok(Some(byte)),
            Err(crate::slice_input_buffer::Error::ReachedEnd) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArrayBitStack, BitStackStruct, String};

    #[test]
    fn make_parser() {
        let input = r#"{"key": "value"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(input, &mut scratch);
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
        let input = r#"{"key": 124}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(input, &mut scratch);
        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
        // Check number value using new JsonNumber API
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                assert_eq!(num.as_str(), "124");
                assert_eq!(num.as_int(), Some(124));
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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);
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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);
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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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

    #[test]
    fn test_original_parser_escape_trace() {
        // Test escape sequence processing with logging
        let input = r#""a\nb""#;
        let mut scratch = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(input, &mut scratch);

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

    #[test]
    fn make_parser_from_slice() {
        let input = br#"{"key": "value"}"#;
        let mut scratch = [0u8; 1024];
        let mut parser = SliceParser::with_buffer_from_slice(input, &mut scratch);
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
    fn test_with_config_constructors() {
        // Test with_config constructor (no escapes)
        let json = r#"{"simple": "no_escapes"}"#;
        let mut parser = SliceParser::<BitStackStruct<u64, u16>>::with_config(json);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("simple")))
        );
        assert_eq!(
            parser.next_event(),
            Ok(Event::String(String::Borrowed("no_escapes")))
        );
        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_with_config_and_buffer_constructors() {
        // Test with_config_and_buffer constructor (with escapes)
        let json = r#"{"escaped": "hello\nworld"}"#;
        let mut scratch = [0u8; 64];
        let mut parser =
            SliceParser::<BitStackStruct<u64, u16>>::with_config_and_buffer(json, &mut scratch);

        assert_eq!(parser.next_event(), Ok(Event::StartObject));
        assert_eq!(
            parser.next_event(),
            Ok(Event::Key(String::Borrowed("escaped")))
        );

        if let Ok(Event::String(s)) = parser.next_event() {
            assert_eq!(s.as_ref(), "hello\nworld"); // Escape should be processed
        } else {
            panic!("Expected String event");
        }

        assert_eq!(parser.next_event(), Ok(Event::EndObject));
        assert_eq!(parser.next_event(), Ok(Event::EndDocument));
    }

    #[test]
    fn test_alternative_config_deep_nesting() {
        // Test that custom BitStack configs can handle deeper nesting
        let json = r#"{"a":{"b":{"c":{"d":{"e":"deep"}}}}}"#;
        let mut scratch = [0u8; 64];
        let mut parser =
            SliceParser::<ArrayBitStack<8, u32, u16>>::with_config_and_buffer(json, &mut scratch);

        // Parse the deep structure
        let mut depth = 0;
        while let Ok(event) = parser.next_event() {
            match event {
                Event::StartObject => depth += 1,
                Event::EndObject => depth -= 1,
                Event::EndDocument => break,
                _ => {}
            }
        }

        // Should have successfully parsed a 5-level deep structure
        assert_eq!(depth, 0); // All objects should be closed
    }
}
