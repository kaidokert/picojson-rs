// SPDX-License-Identifier: Apache-2.0

// Push parser tests for the integrated escape handling functionality
#[cfg(test)]
mod tests {
    use picojson::{
        DefaultConfig, Event, ParseError, PullParser, PushParser, PushParserHandler, SliceParser,
    };

    // Simple test handler for the clean implementation
    struct SimpleHandler;

    impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for SimpleHandler {
        fn handle_event(&mut self, _event: Event<'a, 'b>) -> Result<(), ParseError> {
            Ok(())
        }
    }

    #[test]
    fn test_clean_push_parser_compiles() {
        let mut buffer = [0u8; 256];
        let handler = SimpleHandler;
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // This should compile without lifetime issues using HRTB + tokenizer + event array
        parser.write(b"true").unwrap(); // Valid JSON
        let _handler = parser.finish::<ParseError>().unwrap();
    }

    #[test]
    fn test_hrtb_pattern_with_scratch_buffer() {
        // Handler that captures events to verify HRTB works
        struct CapturingHandler {
            event_count: usize,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for CapturingHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                self.event_count += 1;
                match event {
                    Event::String(s) => {
                        // Both String::Borrowed and String::Unescaped should work
                        assert_eq!(s.as_ref(), "hello"); // From input or StreamBuffer via HRTB!
                    }
                    Event::EndDocument => {
                        // Expected
                    }
                    _ => panic!("Unexpected event: {:?}", event),
                }
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = CapturingHandler { event_count: 0 };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test tokenizer + HRTB integration with real JSON
        parser.write(b"\"hello\"").unwrap(); // This should trigger String Begin event -> Unescaped processing
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify events were processed
        assert_eq!(handler.event_count, 2); // String + EndDocument
    }

    #[test]
    fn test_string_borrowed() {
        // Handler that captures strings for verification
        struct StringHandler {
            string_content: Option<std::string::String>, // Use std::string::String to avoid lifetime issues
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for StringHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                match event {
                    Event::String(s) => {
                        // Capture the actual string content for verification
                        self.string_content = Some(s.as_ref().to_owned());
                        Ok(())
                    }
                    Event::EndDocument => Ok(()),
                    _ => Ok(()), // Ignore other events
                }
            }
        }

        let mut buffer = [0u8; 256];
        let handler = StringHandler {
            string_content: None,
        };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test simple string extraction - this should extract "test" from the input
        parser.write(br#""test""#).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // SUCCESS: Verify we extracted the actual string content!
        assert_eq!(
            handler.string_content,
            Some("test".to_owned()),
            "Should extract 'test' from input \"test\""
        );
    }

    #[test]
    fn test_keys() {
        // Debug handler that captures ALL events including keys
        struct KeyTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for KeyTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                let event_desc = match event {
                    Event::StartObject => "StartObject".to_string(),
                    Event::EndObject => "EndObject".to_string(),
                    Event::Key(k) => format!("Key({})", k.as_ref()),
                    Event::String(s) => format!("String({})", s.as_ref()),
                    Event::Bool(b) => format!("Bool({})", b),
                    Event::EndDocument => "EndDocument".to_string(),
                    _ => "Other".to_string(),
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = KeyTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test object with key-value pair
        parser.write(br#"{"name": "value"}"#).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify we captured all object events correctly

        // Should see: [StartObject, Key(name), String(value), EndObject, EndDocument]
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(name)".to_string(),
                "String(value)".to_string(),
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    #[test]
    fn test_simple_escapes() {
        // Debug handler that captures strings and keys to test escape processing
        struct EscapeTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for EscapeTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                let event_desc = match event {
                    Event::StartObject => "StartObject".to_string(),
                    Event::EndObject => "EndObject".to_string(),
                    Event::Key(k) => format!("Key({})", k.as_ref()),
                    Event::String(s) => format!("String({})", s.as_ref()),
                    Event::EndDocument => "EndDocument".to_string(),
                    _ => "Other".to_string(),
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = EscapeTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test string with actual escape sequence (\n should become newline)
        parser.write(b"{\"key\": \"hello\\nworld\"}").unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify escape sequence was processed correctly

        // Should see the escaped newline processed correctly
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(key)".to_string(),
                "String(hello\nworld)".to_string(), // \n in JSON becomes actual newline character
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    #[test]
    fn test_unicode_escapes() {
        // Debug handler that captures strings and keys to test Unicode escape processing
        struct UnicodeEscapeTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for UnicodeEscapeTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                let event_desc = match event {
                    Event::StartObject => "StartObject".to_string(),
                    Event::EndObject => "EndObject".to_string(),
                    Event::Key(k) => format!("Key({})", k.as_ref()),
                    Event::String(s) => format!("String({})", s.as_ref()),
                    Event::EndDocument => "EndDocument".to_string(),
                    _ => "Other".to_string(),
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = UnicodeEscapeTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test string with Unicode escape sequence (\u0041 should become 'A')
        parser.write(br#"{"key": "\u0041"}"#).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify Unicode escape sequence was processed correctly

        // Should see the Unicode escape processed correctly: \u0041 → A
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(key)".to_string(),
                "String(A)".to_string(), // \u0041 should be converted to 'A'
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    #[test]
    fn test_consecutive_unicode_escapes() {
        // Debug handler that captures strings and keys to test consecutive Unicode escapes
        struct ConsecutiveUnicodeTestHandler {
            events: Vec<String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError>
            for ConsecutiveUnicodeTestHandler
        {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                match event {
                    Event::StartObject => self.events.push("StartObject".to_string()),
                    Event::EndObject => self.events.push("EndObject".to_string()),
                    Event::Key(key) => self.events.push(format!("Key({})", key)),
                    Event::String(s) => self.events.push(format!("String({})", s)),
                    Event::EndDocument => self.events.push("EndDocument".to_string()),
                    _ => {}
                }
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = ConsecutiveUnicodeTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test string with mixed escapes like in pass1.json line 45
        parser.write(br#"{"key": "\uCAFE\uBABE"}"#).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify consecutive Unicode escapes were processed correctly

        // Should see both Unicode escapes processed correctly
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(key)".to_string(),
                "String(쫾몾)".to_string(), // \uCAFE\uBABE should be decoded to consecutive Unicode characters
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    // Debug test for tracing PushParser with pass1.json problematic lines
    #[test]
    fn test_push_parser_pass1_specific_lines() {
        struct TraceHandler {
            events: Vec<String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for TraceHandler {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                match event {
                    Event::String(s) => {
                        self.events.push(format!("String({})", s.as_ref()));
                    }
                    Event::Key(key) => {
                        self.events.push(format!("Key({})", key.as_ref()));
                    }
                    _ => {}
                }
                Ok(())
            }
        }

        // Test line 28 from pass1.json first
        let line_28 = r#"{"hex": "\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A"}"#;

        let mut buffer = [0u8; 1024];
        let handler = TraceHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        assert_eq!(parser.write(line_28.as_bytes()), Ok(()));
        assert!(parser.finish::<ParseError>().is_ok());

        // Test line 45 from pass1.json (the longer one we tested before)
        let line_45 = r#""\\/\\\\\\\"\\uCAFE\\uBABE\\uAB98\\uFCDE\\ubcda\\uef4A\\b\\f\\n\\r\\t`1~!@#$%^&*()_+-=[]{}|;:',./<>?""#;

        let mut buffer2 = [0u8; 1024];
        let handler2 = TraceHandler { events: Vec::new() };
        let mut parser2 = PushParser::<_, DefaultConfig>::new(handler2, &mut buffer2);

        assert_eq!(parser2.write(line_45.as_bytes()), Ok(()));
        assert!(parser2.finish::<ParseError>().is_ok());
    }

    // Test larger section of pass1.json to find what causes InvalidSliceBounds
    #[test]
    fn test_push_parser_pass1_larger_section() {
        struct TraceHandler {
            events: Vec<String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for TraceHandler {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                match event {
                    Event::String(s) => {
                        self.events
                            .push(format!("String({} chars)", s.as_ref().len()));
                    }
                    Event::Key(key) => {
                        self.events.push(format!("Key({})", key.as_ref()));
                    }
                    _ => {}
                }
                Ok(())
            }
        }

        // Test a larger section from pass1.json that includes the problematic areas
        #[cfg(feature = "float")]
        let larger_section = r#"{
        "integer": 1234567890,
        "real": -9876.543210,
        "e": 0.123456789e-12,
        "E": 1.234567890E+34,
        "":  23456789012E66,
        "zero": 0,
        "one": 1,
        "space": " ",
        "quote": "\"",
        "backslash": "\\",
        "controls": "\\b\\f\\n\\r\\t",
        "slash": "/ & \/",
        "alpha": "abcdefghijklmnopqrstuvwyz",
        "ALPHA": "ABCDEFGHIJKLMNOPQRSTUVWYZ",
        "digit": "0123456789",
        "0123456789": "digit",
        "special": "`1~!@#$%^&*()_+-={':[,]}|;.</>?",
        "hex": "\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A",
        "true": true,
        "false": false,
        "null": null
    }"#;

        #[cfg(not(feature = "float"))]
        let larger_section = r#"{
        "integer": 1234567890,
        "zero": 0,
        "one": 1,
        "space": " ",
        "quote": "\"",
        "backslash": "\\",
        "controls": "\\b\\f\\n\\r\\t",
        "slash": "/ & \/",
        "alpha": "abcdefghijklmnopqrstuvwyz",
        "ALPHA": "ABCDEFGHIJKLMNOPQRSTUVWYZ",
        "digit": "0123456789",
        "0123456789": "digit",
        "special": "`1~!@#$%^&*()_+-={':[,]}|;.</>?",
        "hex": "\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A",
        "true": true,
        "false": false,
        "null": null
    }"#;

        let mut buffer = [0u8; 2048]; // Larger buffer
        let handler = TraceHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        assert_eq!(parser.write(larger_section.as_bytes()), Ok(()));
        assert!(parser.finish::<ParseError>().is_ok());
    }

    // Test how parsers handle empty keys like in pass1.json
    #[test]
    fn test_empty_key_handling() {
        // Test the exact pattern from pass1.json line 15
        let empty_key_json = r#"{"": 123}"#;

        // Test SliceParser first
        let mut buffer = [0u8; 256];
        let mut slice_parser = SliceParser::with_buffer(empty_key_json, &mut buffer);

        match slice_parser.next_event() {
            Ok(Event::StartObject) => {}
            other => panic!("Expected StartObject event, got {:?}", other),
        }

        match slice_parser.next_event() {
            Ok(Event::Key(k)) => assert_eq!(k.as_ref(), "", "Empty key should be empty string"),
            other => panic!("Expected Key event, got {:?}", other),
        }

        // Test PushParser

        struct EmptyKeyHandler {
            events: Vec<String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for EmptyKeyHandler {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                match event {
                    Event::Key(k) => {
                        self.events.push(format!("Key({})", k.as_ref()));
                    }
                    Event::Number(n) => {
                        self.events.push(format!("Number({})", n.as_str()));
                    }
                    _ => {}
                }
                Ok(())
            }
        }

        let mut buffer2 = [0u8; 256];
        let handler = EmptyKeyHandler { events: Vec::new() };
        let mut push_parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer2);

        assert_eq!(push_parser.write(empty_key_json.as_bytes()), Ok(()));

        let handler = push_parser.finish::<ParseError>().unwrap();
        assert_eq!(
            handler.events,
            vec!["Key()".to_string(), "Number(123)".to_string()],
            "PushParser should capture empty key and number value"
        );
    }

    #[test]
    fn test_numbers() {
        // Debug handler that captures numbers to test number processing
        struct NumberTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for NumberTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
                let event_desc = match event {
                    Event::StartArray => "StartArray".to_string(),
                    Event::EndArray => "EndArray".to_string(),
                    Event::StartObject => "StartObject".to_string(),
                    Event::EndObject => "EndObject".to_string(),
                    Event::Key(k) => format!("Key({})", k.as_ref()),
                    Event::String(s) => format!("String({})", s.as_ref()),
                    Event::Number(n) => format!("Number({})", n.as_str()),
                    Event::Bool(b) => format!("Bool({})", b),
                    Event::Null => "Null".to_string(),
                    Event::EndDocument => "EndDocument".to_string(),
                    // ContentSpan events should not reach user code - they get converted by PushParser
                    Event::ContentSpan { .. }
                    | Event::PartialContentSpanStart { .. }
                    | Event::PartialContentSpanEnd { .. } => {
                        panic!("Internal ContentSpan events should not reach user handlers")
                    }
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = NumberTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test object with various number types
        #[cfg(feature = "float")]
        let json_input = br#"{"int": 42, "float": 3.14, "negative": -123}"#;
        #[cfg(not(feature = "float"))]
        let json_input = br#"{"int": 42, "negative": -123}"#;

        parser.write(json_input).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify number events were captured correctly

        // Should see all number types processed correctly
        #[cfg(feature = "float")]
        let expected = vec![
            "StartObject".to_string(),
            "Key(int)".to_string(),
            "Number(42)".to_string(),
            "Key(float)".to_string(),
            "Number(3.14)".to_string(),
            "Key(negative)".to_string(),
            "Number(-123)".to_string(),
            "EndObject".to_string(),
            "EndDocument".to_string(),
        ];

        #[cfg(not(feature = "float"))]
        let expected = vec![
            "StartObject".to_string(),
            "Key(int)".to_string(),
            "Number(42)".to_string(),
            "Key(negative)".to_string(),
            "Number(-123)".to_string(),
            "EndObject".to_string(),
            "EndDocument".to_string(),
        ];

        assert_eq!(handler.events, expected);
    }

    #[test]
    fn test_single_slash_escape() {
        struct Handler {
            events: Vec<String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                if let Event::String(s) = event {
                    self.events.push(format!("String({})", s));
                }
                Ok(())
            }
        }

        let mut buffer = [0u8; 64];
        let handler = Handler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test just \/
        parser.write(br#""\/""#).unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify single slash escape was processed correctly
        // Should be: ["String(/)"]
        assert_eq!(handler.events, vec!["String(/)".to_string()]);
    }

    #[test]
    fn test_invalid_unicode_escape_incomplete() {
        struct Handler;

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(
                &mut self,
                _event: picojson::Event<'input, 'scratch>,
            ) -> Result<(), ParseError> {
                Ok(())
            }
        }

        let mut buffer = [0u8; 64];
        let handler = Handler;
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test incomplete Unicode escape (missing hex digits)
        let write_result = parser.write(br#""\u004""#);
        if write_result.is_ok() {
            // If write succeeds, the error should be caught in finish
            let finish_result = parser.finish::<ParseError>();
            assert!(
                finish_result.is_err(),
                "Incomplete Unicode escape should fail during finish"
            );
        } else {
            // If write fails, that's also acceptable for incomplete escape
            assert!(
                write_result.is_err(),
                "Incomplete Unicode escape should fail"
            );
        }
    }

    #[test]
    fn test_invalid_unicode_escape_invalid_hex() {
        struct Handler;

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(
                &mut self,
                _event: picojson::Event<'input, 'scratch>,
            ) -> Result<(), ParseError> {
                Ok(())
            }
        }

        let mut buffer = [0u8; 64];
        let handler = Handler;
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test invalid hex character in Unicode escape
        let result = parser.write(br#""\u004G""#);
        assert!(
            result.is_err(),
            "Invalid hex character in Unicode escape should fail"
        );
    }

    #[test]
    fn test_invalid_unicode_escape_in_key() {
        struct Handler;

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(
                &mut self,
                _event: picojson::Event<'input, 'scratch>,
            ) -> Result<(), ParseError> {
                Ok(())
            }
        }

        let mut buffer = [0u8; 64];
        let handler = Handler;
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test invalid Unicode escape in object key
        let result = parser.write(br#"{"\u004Z": "value"}"#);
        assert!(result.is_err(), "Invalid Unicode escape in key should fail");
    }

    #[test]
    fn test_mixed_borrowed_and_unescaped_strings() {
        struct Handler {
            events: Vec<std::string::String>,
        }

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
                match event {
                    Event::String(s) => {
                        let content = s.as_ref().to_string();
                        let string_type = match s {
                            picojson::String::Borrowed(_) => "Borrowed",
                            picojson::String::Unescaped(_) => "Unescaped",
                        };
                        self.events.push(format!("{}({})", string_type, content));
                    }
                    Event::Key(k) => {
                        let content = k.as_ref().to_string();
                        let key_type = match k {
                            picojson::String::Borrowed(_) => "BorrowedKey",
                            picojson::String::Unescaped(_) => "UnescapedKey",
                        };
                        self.events.push(format!("{}({})", key_type, content));
                    }
                    _ => {}
                }
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = Handler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test object with both borrowed (simple) and unescaped (with escapes) strings
        parser
            .write(br#"{"simple": "value", "escaped": "hello\\nworld"}"#)
            .unwrap();
        let handler = parser.finish::<ParseError>().unwrap();

        // Verify we have both borrowed and unescaped string types
        let has_borrowed = handler.events.iter().any(|e| e.starts_with("Borrowed"));
        let has_unescaped = handler.events.iter().any(|e| e.starts_with("Unescaped"));

        assert!(has_borrowed, "Should have at least one borrowed string");
        assert!(has_unescaped, "Should have at least one unescaped string");
    }

    #[test]
    fn test_invalid_escape_sequences_in_keys() {
        struct Handler;

        impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for Handler {
            fn handle_event(
                &mut self,
                _event: picojson::Event<'input, 'scratch>,
            ) -> Result<(), ParseError> {
                Ok(())
            }
        }

        let mut buffer = [0u8; 64];
        let handler = Handler;
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test invalid escape sequence in object key (\x is not valid JSON)
        let result = parser.write(br#"{"\x41": "value"}"#);
        assert!(
            result.is_err(),
            "Invalid escape sequence in key should fail"
        );
    }
}
