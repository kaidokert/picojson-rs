// SPDX-License-Identifier: Apache-2.0

//! Test for PushParser copy-on-escape optimization (no_std compliant)

use picojson::{DefaultConfig, Event, PushParser, PushParserHandler, String};

#[test]
fn test_borrowed_vs_unescaped_simple() {
    // Test simple case: both strings should be borrowed (no escapes)
    struct SimpleHandler {
        key_is_borrowed: Option<bool>,
        value_is_borrowed: Option<bool>,
    }

    impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ()> for SimpleHandler {
        fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ()> {
            match event {
                Event::Key(s) => {
                    self.key_is_borrowed = Some(matches!(s, String::Borrowed(_)));
                }
                Event::String(s) => {
                    self.value_is_borrowed = Some(matches!(s, String::Borrowed(_)));
                }
                _ => {}
            }
            Ok(())
        }
    }

    let mut buffer = [0u8; 1024];
    let handler = SimpleHandler {
        key_is_borrowed: None,
        value_is_borrowed: None,
    };
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    parser.write(br#"{"foo": "bar"}"#).unwrap();
    parser.finish().unwrap();

    let handler = parser.destroy();

    // Both should be borrowed since no escapes
    assert_eq!(
        handler.key_is_borrowed,
        Some(true),
        "Key 'foo' should be String::Borrowed"
    );
    assert_eq!(
        handler.value_is_borrowed,
        Some(true),
        "Value 'bar' should be String::Borrowed"
    );
}

#[test]
fn test_borrowed_vs_unescaped_with_escapes() {
    // Test with escapes: should be unescaped
    struct EscapeHandler {
        key_is_borrowed: Option<bool>,
        value_is_borrowed: Option<bool>,
    }

    impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ()> for EscapeHandler {
        fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ()> {
            match event {
                Event::Key(s) => {
                    self.key_is_borrowed = Some(matches!(s, String::Borrowed(_)));
                }
                Event::String(s) => {
                    self.value_is_borrowed = Some(matches!(s, String::Borrowed(_)));
                }
                _ => {}
            }
            Ok(())
        }
    }

    let mut buffer = [0u8; 1024];
    let handler = EscapeHandler {
        key_is_borrowed: None,
        value_is_borrowed: None,
    };
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    parser.write(br#"{"key\\n": "val\\t"}"#).unwrap();
    parser.finish().unwrap();

    let handler = parser.destroy();

    // Both should be unescaped since they have escape sequences
    assert_eq!(
        handler.key_is_borrowed,
        Some(false),
        "Key with escape should be String::Unescaped"
    );
    assert_eq!(
        handler.value_is_borrowed,
        Some(false),
        "Value with escape should be String::Unescaped"
    );
}

#[test]
fn test_buffer_isolation() {
    // Test that strings don't accumulate content from previous strings
    struct ContentChecker {
        first_string: Option<[u8; 32]>,
        first_len: usize,
        second_string: Option<[u8; 32]>,
        second_len: usize,
        count: usize,
    }

    impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ()> for ContentChecker {
        fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ()> {
            match event {
                Event::Key(s) | Event::String(s) => {
                    let bytes = s.as_ref().as_bytes();
                    if self.count == 0 {
                        // First string
                        let mut buf = [0u8; 32];
                        let len = bytes.len().min(32);
                        buf[..len].copy_from_slice(&bytes[..len]);
                        self.first_string = Some(buf);
                        self.first_len = len;
                    } else if self.count == 1 {
                        // Second string
                        let mut buf = [0u8; 32];
                        let len = bytes.len().min(32);
                        buf[..len].copy_from_slice(&bytes[..len]);
                        self.second_string = Some(buf);
                        self.second_len = len;
                    }
                    self.count += 1;
                }
                _ => {}
            }
            Ok(())
        }
    }

    let mut buffer = [0u8; 1024];
    let handler = ContentChecker {
        first_string: None,
        first_len: 0,
        second_string: None,
        second_len: 0,
        count: 0,
    };
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Test: simple string followed by escaped string
    parser.write(br#"{"simple": "esc\\n"}"#).unwrap();
    parser.finish().unwrap();

    let handler = parser.destroy();

    // Verify first string is "simple"
    assert!(handler.first_string.is_some());
    let first = &handler.first_string.unwrap()[..handler.first_len];
    assert_eq!(first, b"simple", "First string should be 'simple'");

    // TODO: Verify second string is "esc\n" (with actual newline) when escape processing is fully working
    // Currently in this test context, escape sequences are not being processed
    assert!(handler.second_string.is_some());
    let second = &handler.second_string.unwrap()[..handler.second_len];
    assert_eq!(
        second, b"esc\\n",
        "Second string should be 'esc\\\\n' (currently literal, to be fixed in Issue #3)"
    );
}
