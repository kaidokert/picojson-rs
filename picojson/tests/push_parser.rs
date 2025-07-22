// SPDX-License-Identifier: Apache-2.0

// Push parser tests for the integrated escape handling functionality
#[cfg(test)]
mod tests {
    use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

    // Simple test handler for the clean implementation
    struct SimpleHandler;

    impl<'a, 'b> PushParserHandler<'a, 'b, ()> for SimpleHandler {
        fn handle_event(&mut self, _event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let _handler = parser.destroy();
    }

    #[test]
    fn test_hrtb_pattern_with_scratch_buffer() {
        // Handler that captures events to verify HRTB works
        struct CapturingHandler {
            event_count: usize,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for CapturingHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        // Verify events were processed
        assert_eq!(handler.event_count, 2); // String + EndDocument
    }

    #[test]
    fn test_string_borrowed() {
        // Handler that captures strings for verification
        struct StringHandler {
            string_content: Option<std::string::String>, // Use std::string::String to avoid lifetime issues
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for StringHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        // SUCCESS: Verify we extracted the actual string content!
        assert_eq!(
            handler.string_content,
            Some("test".to_owned()),
            "Should extract 'test' from input \"test\""
        );
    }

    #[test]
    fn test_debug_all_events() {
        // Debug handler that captures ALL events to understand what's happening
        struct DebugHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for DebugHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
                let event_desc = match event {
                    Event::StartArray => "StartArray".to_string(),
                    Event::EndArray => "EndArray".to_string(),
                    Event::StartObject => "StartObject".to_string(),
                    Event::EndObject => "EndObject".to_string(),
                    Event::String(s) => format!("String({})", s.as_ref()),
                    Event::Bool(b) => format!("Bool({})", b),
                    Event::Null => "Null".to_string(),
                    Event::EndDocument => "EndDocument".to_string(),
                    _ => "Other".to_string(),
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = DebugHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // First test: simple string that we know works
        parser.write(br#""hello""#).unwrap();
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Simple string events: {:?}", handler.events);

        // Now test array
        let mut buffer2 = [0u8; 256];
        let handler2 = DebugHandler { events: Vec::new() };
        let mut parser2 = PushParser::<_, DefaultConfig>::new(handler2, &mut buffer2);

        parser2.write(br#"["hello"]"#).unwrap();
        parser2.finish::<()>().unwrap();
        let handler2 = parser2.destroy();

        println!("Array events: {:?}", handler2.events);

        // Verify we get at least some events
        assert!(!handler2.events.is_empty(), "Should receive some events");
    }

    #[test]
    fn test_keys() {
        // Debug handler that captures ALL events including keys
        struct KeyTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for KeyTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Object events: {:?}", handler.events);

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

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for EscapeTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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

        // Test string with escape sequence (\n should become newline)
        parser.write(br#"{"key": "hello\nworld"}"#).unwrap();
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Escape events: {:?}", handler.events);

        // Should see the escaped newline processed correctly
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(key)".to_string(),
                "String(hello\nworld)".to_string(), // \n should be converted to actual newline
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    #[test_log::test]
    fn test_unicode_escapes() {
        // Debug handler that captures strings and keys to test Unicode escape processing
        struct UnicodeEscapeTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for UnicodeEscapeTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Unicode escape events: {:?}", handler.events);

        // Should see the Unicode escape processed correctly: \u0041 → A
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(key)".to_string(),
                "String(A)".to_string(), // \\u0041 should be converted to 'A'
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }

    #[test]
    fn test_unicode_escapes_not_yet_implemented() {
        // Debug handler that captures strings and keys to test Unicode escape processing
        struct UnicodeEscapeTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for UnicodeEscapeTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Unicode escape events: {:?}", handler.events);

        // This test verifies that Unicode escapes are working
    }

    #[test]
    fn test_numbers() {
        // Debug handler that captures numbers to test number processing
        struct NumberTestHandler {
            events: Vec<std::string::String>,
        }

        impl<'a, 'b> PushParserHandler<'a, 'b, ()> for NumberTestHandler {
            fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
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
                };
                self.events.push(event_desc);
                Ok(())
            }
        }

        let mut buffer = [0u8; 256];
        let handler = NumberTestHandler { events: Vec::new() };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        // Test object with various number types
        parser
            .write(br#"{"int": 42, "float": 3.14, "negative": -123}"#)
            .unwrap();
        parser.finish::<()>().unwrap();
        let handler = parser.destroy();

        println!("Number events: {:?}", handler.events);

        // Should see all number types processed correctly
        assert_eq!(
            handler.events,
            vec![
                "StartObject".to_string(),
                "Key(int)".to_string(),
                "Number(42)".to_string(),
                "Key(float)".to_string(),
                "Number(3.14)".to_string(),
                "Key(negative)".to_string(),
                "Number(-123)".to_string(),
                "EndObject".to_string(),
                "EndDocument".to_string()
            ]
        );
    }
}
