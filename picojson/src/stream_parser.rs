// SPDX-License-Identifier: Apache-2.0

use crate::event_processor::{ContentExtractor, EscapeHandler, EscapeTiming, ParserCore};
use crate::parse_error::ParseError;
use crate::shared::{Event, State};
use crate::stream_content_builder::StreamContentBuilder;
use crate::{ujson, PullParser};

use ujson::{BitStackConfig, DefaultConfig};

/// Trait for input sources that can provide data to the streaming parser.
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

/// A pull parser that parses JSON from a stream.
///
/// Generic over BitStackConfig for configurable nesting depth.
/// It is designed to be used with the [Reader] trait, which is used to read data from a stream.
///
pub struct StreamParser<'b, R: Reader, C: BitStackConfig = DefaultConfig> {
    /// The shared parser core that handles the unified event processing loop
    parser_core: ParserCore<C::Bucket, C::Counter>,
    /// The unified provider that handles both content building and reader access
    /// This allows us to use the same unified pattern as SliceParser
    provider: StreamParserProvider<'b, R>,
}

/// Methods for StreamParser using DefaultConfig
impl<'b, R: Reader> StreamParser<'b, R, DefaultConfig> {
    /// Create a new StreamParser with default configuration
    ///
    /// Uses the default BitStack configuration (u32 bucket, u8 counter)
    /// for most common use cases.
    pub fn new(reader: R, buffer: &'b mut [u8]) -> Self {
        Self::with_config(reader, buffer)
    }
}

/// Methods for StreamParser with custom BitStackConfig
impl<'b, R: Reader, C: BitStackConfig> StreamParser<'b, R, C> {
    /// Create a new StreamParser with custom configuration
    ///
    /// Use this when you need custom BitStack storage types for specific
    /// memory or nesting depth requirements.
    ///
    /// # Example
    /// ```
    /// use picojson::{StreamParser, BitStackStruct, ChunkReader};
    ///
    /// let json = b"{\"test\": 42}";
    /// // For testing, ChunkReader is a convenient Reader implementation
    /// let reader = ChunkReader::new(json, 10);
    /// let mut buffer = [0u8; 256];
    ///
    /// // Custom configuration: u64 bucket + u16 counter for deeper nesting
    /// let mut parser = StreamParser::<_, BitStackStruct<u64, u16>>::with_config(reader, &mut buffer);
    /// ```
    pub fn with_config(reader: R, buffer: &'b mut [u8]) -> Self {
        Self {
            parser_core: ParserCore::new(),
            provider: StreamParserProvider::new(reader, buffer),
        }
    }
}

/// Implement the required traits for StreamParser to work with unified ParserCore
/// This provider handles the StreamParser-specific operations needed by the unified parser core
/// It bridges the gap between the generic ParserCore and the StreamParser's specific requirements
/// for streaming input and buffer management
/// The provider contains mutable references to the StreamParser's internal state
/// which allows the unified parser core to control the parsing process
pub struct StreamParserProvider<'b, R: Reader> {
    content_builder: StreamContentBuilder<'b>,
    reader: R,
    finished: bool,
}

impl<'b, R: Reader> StreamParserProvider<'b, R> {
    pub fn new(reader: R, buffer: &'b mut [u8]) -> Self {
        Self {
            content_builder: StreamContentBuilder::new(buffer),
            reader,
            finished: false,
        }
    }
}

impl<R: Reader> EscapeHandler for StreamParserProvider<'_, R> {
    fn parser_state(&self) -> &State {
        self.content_builder.parser_state()
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        self.content_builder.process_unicode_escape_with_collector()
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        self.content_builder.handle_simple_escape_char(escape_char)
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        self.content_builder.begin_escape_sequence()
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        self.content_builder.begin_unicode_escape()
    }
}

impl<R: Reader> ContentExtractor for StreamParserProvider<'_, R> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        // If buffer is empty, try to fill it first
        if self.content_builder.stream_buffer().is_empty() {
            self.content_builder
                .fill_buffer_from_reader(&mut self.reader)?;
        }

        // If still empty after fill attempt, we're at EOF
        if self.content_builder.stream_buffer().is_empty() {
            if !self.finished {
                self.finished = true;
                self.content_builder.set_finished(true);
            }
            return Ok(None);
        }

        // Get byte and advance
        let byte = self.content_builder.stream_buffer().current_byte()?;
        self.content_builder.stream_buffer_mut().advance()?;
        Ok(Some(byte))
    }

    fn parser_state_mut(&mut self) -> &mut State {
        self.content_builder.parser_state_mut()
    }

    fn current_position(&self) -> usize {
        self.content_builder.current_position()
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.content_builder.begin_string_content(pos);
    }

    fn unicode_escape_collector_mut(
        &mut self,
    ) -> &mut crate::escape_processor::UnicodeEscapeCollector {
        self.content_builder.unicode_escape_collector_mut()
    }

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        self.content_builder.extract_string_content(start_pos)
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        self.content_builder.extract_key_content(start_pos)
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        self.content_builder
            .extract_number(start_pos, from_container_end, finished)
    }

    /// Override the default validate_and_extract_number to use the finished state
    fn validate_and_extract_number(
        &mut self,
        from_container_end: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        let start_pos = match *self.parser_state() {
            crate::shared::State::Number(pos) => pos,
            _ => return Err(crate::shared::UnexpectedState::StateMismatch.into()),
        };

        *self.parser_state_mut() = crate::shared::State::None;
        // Use the finished-aware extract_number method
        self.extract_number(start_pos, from_container_end, self.finished)
    }
}

/// Shared methods for StreamParser with any BitStackConfig
impl<R: Reader, C: BitStackConfig> StreamParser<'_, R, C> {
    /// Get the next JSON event from the stream
    fn next_event_impl(&mut self) -> Result<Event<'_, '_>, ParseError> {
        // Use the unified ParserCore implementation with StreamParser-specific timing
        // StreamParser needs byte accumulation for string parsing
        self.parser_core.next_event_impl(
            &mut self.provider,
            EscapeTiming::OnEnd,
            |provider, byte| {
                // Delegate to the StreamContentBuilder's byte accumulation logic
                provider.content_builder.handle_byte_accumulation(byte)
            },
        )
    }

    // The compaction and helper methods are now handled by the provider
    // These methods can be removed since they're not needed with the new architecture
}

impl<R: Reader, C: BitStackConfig> PullParser for StreamParser<'_, R, C> {
    fn next_event(&mut self) -> Result<Event<'_, '_>, ParseError> {
        // Check if we're already finished (similar to SliceParser's is_past_end check)
        if self.provider.finished {
            return Ok(Event::EndDocument);
        }

        self.provider
            .content_builder
            .apply_unescaped_reset_if_queued();

        self.next_event_impl()
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

    impl Reader for SliceReader<'_> {
        type Error = ();

        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let remaining = self.data.len().saturating_sub(self.position);
            if remaining == 0 {
                return Ok(0); // EOF
            }

            let to_copy = remaining.min(buf.len());
            let end_pos = self.position.saturating_add(to_copy);
            if let (Some(dest), Some(src)) = (
                buf.get_mut(..to_copy),
                self.data.get(self.position..end_pos),
            ) {
                dest.copy_from_slice(src);
                self.position = end_pos;
                Ok(to_copy)
            } else {
                Err(())
            }
        }
    }

    type TestStreamParser<'b> = StreamParser<'b, SliceReader<'static>>;

    #[test]
    fn test_direct_parser_simple_object() {
        let json = b"{}";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            assert_eq!(json_string.as_str(), "hello\nworld");
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
        let mut parser1 = TestStreamParser::new(reader1, &mut buffer1);

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
        let mut parser2 = TestStreamParser::new(reader2, &mut buffer2);

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
        let mut parser3 = TestStreamParser::new(reader3, &mut buffer3);

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
        let mut parser4 = TestStreamParser::new(reader4, &mut buffer4);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser1 = TestStreamParser::new(reader1, &mut buffer1);

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
        let mut parser2 = TestStreamParser::new(reader2, &mut buffer2);

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
        let mut parser3 = TestStreamParser::new(reader3, &mut buffer3);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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

    #[test]
    fn test_direct_parser_array_of_strings() {
        let json = b"[\"first\", \"second\"]";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            let content = json_string.as_str();
            // Check that escape sequences were properly processed
            let has_newline = content.contains('\n');
            let has_tab = content.contains('\t');
            let has_quote = content.contains('"');

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

        if let Event::String(json_string) = parser.next_event().unwrap() {
            let content = json_string.as_str();
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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartArray);
        assert_eq!(parser.next_event().unwrap(), Event::Bool(true));
        assert_eq!(parser.next_event().unwrap(), Event::Bool(false));
        assert_eq!(parser.next_event().unwrap(), Event::Null);
        assert_eq!(parser.next_event().unwrap(), Event::EndArray);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_number_simple() {
        let json = b"42";
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
        let mut parser = TestStreamParser::new(reader, &mut buffer);

        #[cfg(feature = "float-error")]
        {
            // float-error configuration should return an error for float values
            let result = parser.next_event();
            assert!(
                result.is_err(),
                "Expected error for float with float-error configuration"
            );
            return;
        }

        #[cfg(not(feature = "float-error"))]
        {
            let event = parser.next_event().unwrap();
            if let Event::Number(json_number) = event {
                assert_eq!(json_number.as_str(), "3.14159");
            } else {
                panic!("Expected Number event, got: {:?}", event);
            }

            let event = parser.next_event().unwrap();
            assert_eq!(event, Event::EndDocument);
        }
    }

    #[test]
    fn test_direct_parser_numbers_in_array() {
        #[cfg(feature = "float-error")]
        let json = b"[42, -7]"; // No floats for float-error config
        #[cfg(not(feature = "float-error"))]
        let json = b"[42, -7, 3.14]"; // Include float for other configs

        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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

        #[cfg(not(feature = "float-error"))]
        {
            let event = parser.next_event().unwrap();
            if let Event::Number(json_number) = event {
                assert_eq!(json_number.as_str(), "3.14");
            } else {
                panic!("Expected Number event, got: {:?}", event);
            }
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndArray);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_numbers_in_object() {
        #[cfg(feature = "float-error")]
        let json = b"{\"count\": 42, \"score\": -7}"; // No floats for float-error config
        #[cfg(not(feature = "float-error"))]
        let json = b"{\"count\": 42, \"score\": -7.5}"; // Include float for other configs

        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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
            #[cfg(feature = "float-error")]
            assert_eq!(val2.as_str(), "-7");
            #[cfg(not(feature = "float-error"))]
            assert_eq!(val2.as_str(), "-7.5");
        } else {
            panic!("Expected Number event");
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_direct_parser_no_float_configuration() {
        // Test that StreamParser properly uses unified number parsing with no-float config
        let json = br#"{"integer": 42, "float": 3.14, "scientific": 1e3}"#;
        let reader = SliceReader::new(json);
        let mut buffer = [0u8; 256];
        let mut parser = TestStreamParser::new(reader, &mut buffer);

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

        // Float key-value - behavior varies by configuration
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("float"))
        );

        #[cfg(feature = "float-error")]
        {
            // float-error should return an error when encountering floats
            let result = parser.next_event();
            assert!(
                result.is_err(),
                "Expected error for float with float-error configuration"
            );
            // Test ends here for float-error - no more processing needed
        }

        #[cfg(not(feature = "float-error"))]
        {
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
                        assert!((f - 3.14).abs() < 0.01);
                    }
                    #[cfg(feature = "float-skip")]
                    crate::NumberResult::FloatSkipped => {
                        // This is expected in float-skip build
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

            // Scientific notation handling varies by float configuration
            assert_eq!(
                parser.next_event().unwrap(),
                Event::Key(crate::String::Borrowed("scientific"))
            );

            // float-truncate rejects scientific notation, so test should end early for that config
            #[cfg(feature = "float-truncate")]
            {
                // float-truncate rejects scientific notation since it would require float math
                let result = parser.next_event();
                assert!(
                    result.is_err(),
                    "Expected error for scientific notation with float-truncate"
                );
                // Test ends here for float-truncate - no more processing needed
            }

            #[cfg(not(feature = "float-truncate"))]
            {
                if let Event::Number(num) = parser.next_event().unwrap() {
                    assert_eq!(num.as_str(), "1e3");
                    match num.parsed() {
                        #[cfg(not(feature = "float"))]
                        crate::NumberResult::FloatDisabled => {
                            // This is expected in no-float build - raw string preserved for manual parsing
                        }
                        #[cfg(feature = "float-skip")]
                        crate::NumberResult::FloatSkipped => {
                            // This is expected in float-skip build
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
    }

    #[test]
    fn test_number_parsing_delimiter_exclusion() {
        // Test that numbers don't include trailing delimiters in various contexts

        // Test 1: Number followed by array end
        let json1 = b"[123]";
        let reader1 = SliceReader::new(json1);
        let mut buffer1 = [0u8; 256];
        let mut parser1 = TestStreamParser::new(reader1, &mut buffer1);

        assert!(matches!(parser1.next_event().unwrap(), Event::StartArray));
        if let Event::Number(num) = parser1.next_event().unwrap() {
            assert_eq!(
                num.as_str(),
                "123",
                "Number should not include trailing delimiter ']'"
            );
        } else {
            panic!("Expected Number event");
        }
        assert!(matches!(parser1.next_event().unwrap(), Event::EndArray));

        // Test 2: Number followed by object end
        let json2 = b"{\"key\":456}";
        let reader2 = SliceReader::new(json2);
        let mut buffer2 = [0u8; 256];
        let mut parser2 = TestStreamParser::new(reader2, &mut buffer2);

        assert!(matches!(parser2.next_event().unwrap(), Event::StartObject));
        assert!(matches!(parser2.next_event().unwrap(), Event::Key(_)));
        if let Event::Number(num) = parser2.next_event().unwrap() {
            assert_eq!(
                num.as_str(),
                "456",
                "Number should not include trailing delimiter '}}'"
            );
        } else {
            panic!("Expected Number event");
        }
        assert!(matches!(parser2.next_event().unwrap(), Event::EndObject));

        // Test 3: Number followed by comma in array
        let json3 = b"[789,10]";
        let reader3 = SliceReader::new(json3);
        let mut buffer3 = [0u8; 256];
        let mut parser3 = TestStreamParser::new(reader3, &mut buffer3);

        assert!(matches!(parser3.next_event().unwrap(), Event::StartArray));
        if let Event::Number(num1) = parser3.next_event().unwrap() {
            assert_eq!(
                num1.as_str(),
                "789",
                "First number should not include trailing delimiter ','"
            );
        } else {
            panic!("Expected first Number event");
        }
        if let Event::Number(num2) = parser3.next_event().unwrap() {
            assert_eq!(
                num2.as_str(),
                "10",
                "Second number should not include trailing delimiter ']'"
            );
        } else {
            panic!("Expected second Number event");
        }
        assert!(matches!(parser3.next_event().unwrap(), Event::EndArray));

        // Test 4: Number followed by comma in object
        let json4 = b"{\"a\":11,\"b\":22}";
        let reader4 = SliceReader::new(json4);
        let mut buffer4 = [0u8; 256];
        let mut parser4 = TestStreamParser::new(reader4, &mut buffer4);

        assert!(matches!(parser4.next_event().unwrap(), Event::StartObject));
        assert!(matches!(parser4.next_event().unwrap(), Event::Key(_)));
        if let Event::Number(num1) = parser4.next_event().unwrap() {
            assert_eq!(
                num1.as_str(),
                "11",
                "First number should not include trailing delimiter ','"
            );
        } else {
            panic!("Expected first Number event");
        }
        assert!(matches!(parser4.next_event().unwrap(), Event::Key(_)));
        if let Event::Number(num2) = parser4.next_event().unwrap() {
            assert_eq!(
                num2.as_str(),
                "22",
                "Second number should not include trailing delimiter '}}'"
            );
        } else {
            panic!("Expected second Number event");
        }
        assert!(matches!(parser4.next_event().unwrap(), Event::EndObject));

        // Test 5: Standalone number at end of document (should include full content)
        let json5 = b"999";
        let reader5 = SliceReader::new(json5);
        let mut buffer5 = [0u8; 256];
        let mut parser5 = TestStreamParser::new(reader5, &mut buffer5);

        if let Event::Number(num) = parser5.next_event().unwrap() {
            assert_eq!(
                num.as_str(),
                "999",
                "Standalone number should include full content"
            );
        } else {
            panic!("Expected Number event");
        }
        assert!(matches!(parser5.next_event().unwrap(), Event::EndDocument));

        // Test 6: Negative numbers with delimiters
        let json6 = b"[-42,33]";
        let reader6 = SliceReader::new(json6);
        let mut buffer6 = [0u8; 256];
        let mut parser6 = TestStreamParser::new(reader6, &mut buffer6);

        assert!(matches!(parser6.next_event().unwrap(), Event::StartArray));
        if let Event::Number(num1) = parser6.next_event().unwrap() {
            assert_eq!(
                num1.as_str(),
                "-42",
                "Negative number should not include trailing delimiter ','"
            );
        } else {
            panic!("Expected first Number event");
        }
        if let Event::Number(num2) = parser6.next_event().unwrap() {
            assert_eq!(
                num2.as_str(),
                "33",
                "Second number should not include trailing delimiter ']'"
            );
        } else {
            panic!("Expected second Number event");
        }
        assert!(matches!(parser6.next_event().unwrap(), Event::EndArray));

        // Test 7: Decimal numbers with delimiters (if float enabled)
        #[cfg(not(feature = "float-error"))]
        {
            let json7 = b"[3.14,2.71]";
            let reader7 = SliceReader::new(json7);
            let mut buffer7 = [0u8; 256];
            let mut parser7 = TestStreamParser::new(reader7, &mut buffer7);

            assert!(matches!(parser7.next_event().unwrap(), Event::StartArray));
            if let Event::Number(num1) = parser7.next_event().unwrap() {
                assert_eq!(
                    num1.as_str(),
                    "3.14",
                    "Decimal number should not include trailing delimiter ','"
                );
            } else {
                panic!("Expected first Number event");
            }
            if let Event::Number(num2) = parser7.next_event().unwrap() {
                assert_eq!(
                    num2.as_str(),
                    "2.71",
                    "Second decimal number should not include trailing delimiter ']'"
                );
            } else {
                panic!("Expected second Number event");
            }
            assert!(matches!(parser7.next_event().unwrap(), Event::EndArray));
        }
    }

    #[test]
    fn test_escape_buffer_functions() {
        // Test the uncovered escape processing functions
        let json_stream = br#"{"escaped": "test\nstring"}"#;
        let mut buffer = [0u8; 1024];
        let mut parser = StreamParser::new(SliceReader::new(json_stream), &mut buffer);

        // These functions are private but we can test them through the public API
        // The escape processing should trigger the uncovered functions
        assert_eq!(parser.next_event().unwrap(), Event::StartObject);
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("escaped"))
        );

        // This should trigger append_byte_to_escape_buffer and queue_unescaped_reset
        if let Event::String(s) = parser.next_event().unwrap() {
            assert_eq!(s.as_ref(), "test\nstring"); // Escape sequence should be processed
        } else {
            panic!("Expected String event with escape sequence");
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_slice_reader_constructor() {
        // Test the uncovered SliceReader::new function
        let data = b"test data";
        let reader = SliceReader::new(data);
        assert_eq!(reader.data, data);
        assert_eq!(reader.position, 0);
    }

    #[test]
    fn test_complex_escape_sequences() {
        // Test more complex escape processing to cover the escape buffer functions
        let json_stream = br#"{"multi": "line1\nline2\ttab\r\n"}"#;
        let mut buffer = [0u8; 1024];
        let mut parser = StreamParser::new(SliceReader::new(json_stream), &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartObject);
        assert_eq!(
            parser.next_event().unwrap(),
            Event::Key(crate::String::Borrowed("multi"))
        );

        // This should exercise the escape buffer processing extensively
        if let Event::String(s) = parser.next_event().unwrap() {
            assert_eq!(s.as_ref(), "line1\nline2\ttab\r\n");
        } else {
            panic!("Expected String event");
        }

        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    /// Helper function to test escape sequence parsing with specific buffer size
    fn test_simple_escape_with_buffer_size(buffer_size: usize) -> Result<(), crate::ParseError> {
        // DEBUG TEST: Simple escape sequence should need minimal buffer
        // JSON: ["hello\\"] = 10 bytes total, should work with ~7 byte buffer
        let json_stream = br#"["hello\\"]"#;

        let mut buffer = vec![0u8; buffer_size];
        let mut parser = StreamParser::new(SliceReader::new(json_stream), &mut buffer);

        // Array start
        assert!(matches!(parser.next_event()?, Event::StartArray));

        // String with escape
        assert!(matches!(parser.next_event()?, Event::String(s) if s.as_str() == "hello\\"));

        // Array end
        assert!(matches!(parser.next_event()?, Event::EndArray));

        // End document
        assert!(matches!(parser.next_event()?, Event::EndDocument));

        Ok(())
    }

    #[test]
    fn test_minimal_buffer_simple_escape_1() {
        // Buffer size 4 - clearly not enough
        assert!(matches!(
            test_simple_escape_with_buffer_size(4),
            Err(crate::ParseError::ScratchBufferFull)
        ));
    }

    #[test]
    fn test_minimal_buffer_simple_escape_2() {
        // Buffer size 12 - test if larger buffer avoids compaction bugs
        test_simple_escape_with_buffer_size(12)
            .expect("12-byte buffer should be sufficient for simple escape");
    }

    #[test]
    fn test_minimal_buffer_simple_escape_3() {
        // Buffer size 24 - known working boundary from stress tests
        test_simple_escape_with_buffer_size(24).expect("24-byte buffer should definitely work");
    }

    #[test]
    fn test_surrogate_pair_buffer_boundary_cases() {
        // Test 7: Surrogate pair split across very small buffer chunks
        let input7 = r#"["\uD801\uDC37"]"#;
        let mut buffer7 = [0u8; 16]; // Small buffer
        let reader7 = crate::chunk_reader::ChunkReader::new(input7.as_bytes(), 3); // Tiny chunks
        let mut parser7 = StreamParser::<_, DefaultConfig>::with_config(reader7, &mut buffer7);
        assert_eq!(parser7.next_event(), Ok(Event::StartArray));
        match parser7.next_event() {
            Ok(Event::String(s)) => {
                let content = match s {
                    crate::String::Borrowed(c) => c,
                    crate::String::Unescaped(c) => c,
                };
                assert_eq!(content, "𐐷");
            }
            other => panic!(
                "Expected String with surrogate pair across buffer boundary, got: {:?}",
                other
            ),
        }

        // Test 8: Surrogate pair with small buffer (still needs minimum space)
        let input8 = r#"["\uD801\uDC37"]"#;
        let mut buffer8 = [0u8; 32]; // Small but sufficient buffer
        let reader8 = crate::chunk_reader::ChunkReader::new(input8.as_bytes(), 6); // Split at surrogate boundary
        let mut parser8 = StreamParser::<_, DefaultConfig>::with_config(reader8, &mut buffer8);
        assert_eq!(parser8.next_event(), Ok(Event::StartArray));
        match parser8.next_event() {
            Ok(Event::String(s)) => {
                let content = match s {
                    crate::String::Borrowed(c) => c,
                    crate::String::Unescaped(c) => c,
                };
                assert_eq!(content, "𐐷");
            }
            other => panic!(
                "Expected String with surrogate pair at small buffer, got: {:?}",
                other
            ),
        }
    }
}
