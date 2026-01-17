// Additional error handling tests for the API

use picojson::{Event, ParseError, PullParser, SliceParser, String};

#[test]
fn test_malformed_json_missing_quotes() {
    let json = r#"{name: "value"}"#; // Missing quotes around key
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));

    // Should fail when parsing the unquoted key
    match parser.next_event() {
        Err(ParseError::TokenizerError(_)) => {
            // Expected - tokenizer should reject unquoted keys
        }
        other => panic!("Expected TokenizerError for unquoted key, got: {:?}", other),
    }
}

#[test]
fn test_malformed_json_unterminated_string() {
    let json = r#"{"unterminated": "missing quote}"#; // Missing closing quote
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("unterminated")))
    );

    // Should fail when trying to parse the unterminated string
    match parser.next_event() {
        Err(ParseError::TokenizerError(_)) => {
            // Expected behavior
        }
        other => panic!(
            "Expected TokenizerError for unterminated string, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_malformed_json_invalid_escape() {
    let json = r#"{"bad_escape": "invalid\x"}"#; // Invalid escape sequence
    let mut scratch = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("bad_escape")))
    );

    // Should fail on invalid escape sequence
    match parser.next_event() {
        Err(ParseError::InvalidEscapeSequence) => {
            // Expected behavior
        }
        Err(ParseError::TokenizerError(_)) => {
            // Also acceptable - tokenizer might catch this first
        }
        other => panic!("Expected escape sequence error, got: {:?}", other),
    }
}

#[test]
fn test_malformed_json_invalid_unicode_escape() {
    let json = r#"{"bad_unicode": "test\uXYZ"}"#; // Invalid Unicode hex
    let mut scratch = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("bad_unicode")))
    );

    // Should fail on invalid Unicode escape
    match parser.next_event() {
        Err(ParseError::InvalidUnicodeHex) => {
            // Expected behavior
        }
        Err(ParseError::TokenizerError(_)) => {
            // Also acceptable - tokenizer might catch this first
        }
        other => panic!("Expected Unicode hex error, got: {:?}", other),
    }
}

#[test]
fn test_buffer_overflow_error() {
    let json = r#"{"large_string": "This is a very long string with escapes\nand more escapes\tand even more content that might overflow a small buffer"}"#;
    let mut small_scratch = [0u8; 10]; // Deliberately small buffer
    let mut parser = SliceParser::with_buffer(json, &mut small_scratch);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("large_string")))
    );

    // Should fail due to insufficient scratch buffer space
    match parser.next_event() {
        Err(ParseError::ScratchBufferFull) => {
            // Expected behavior
        }
        other => panic!("Expected ScratchBufferFull error, got: {:?}", other),
    }
}

#[test]
fn test_empty_input_error() {
    let json = "";
    let mut parser = SliceParser::new(json);

    // Should handle empty input gracefully
    match parser.next_event() {
        Ok(Event::EndDocument) => {
            // This is acceptable - empty input could be treated as end
        }
        Err(ParseError::EndOfData) => {
            // This is also acceptable
        }
        Err(ParseError::TokenizerError(_)) => {
            // This is also acceptable
        }
        other => panic!("Unexpected result for empty input: {:?}", other),
    }
}

#[test]
fn test_incomplete_json_error() {
    let json = r#"{"incomplete""#; // Incomplete JSON
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));

    // Actually parses the key since it's well-formed so far
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("incomplete")))
    );

    // Should fail when trying to find the value or colon
    match parser.next_event() {
        Err(ParseError::TokenizerError(_)) => {
            // Expected behavior when tokenizer hits end unexpectedly
        }
        Err(ParseError::EndOfData) => {
            // Also acceptable
        }
        Ok(Event::EndDocument) => {
            // Parser might be lenient and treat as end
        }
        other => panic!(
            "Expected error or EndDocument for incomplete JSON, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_malformed_json_unexpected_comma() {
    let json = r#"{"key": "value",}"#; // Trailing comma
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
    assert_eq!(
        parser.next_event(),
        Ok(Event::String(String::Borrowed("value")))
    );

    // Parser is lenient with trailing commas, just ends the object
    match parser.next_event() {
        Ok(Event::EndObject) => {
            // Parser accepts trailing comma (lenient behavior)
        }
        Err(ParseError::TokenizerError(_)) => {
            // Strict parser would reject trailing comma
        }
        other => panic!(
            "Expected EndObject or TokenizerError for trailing comma, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_malformed_json_invalid_number() {
    let json = r#"{"number": 123.456.789}"#; // Invalid number format
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("number")))
    );

    // Should fail on invalid number format
    match parser.next_event() {
        Err(ParseError::TokenizerError(_)) => {
            // Expected behavior
        }
        other => panic!(
            "Expected TokenizerError for invalid number, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_error_includes_line_and_column_info() {
    // Test that tokenizer errors propagated through ParseError include line/column info
    let json = "{\n  \"key\": \"value\"\n  invalid\n}";
    let mut parser = SliceParser::new(json);

    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(parser.next_event(), Ok(Event::Key(String::Borrowed("key"))));
    assert_eq!(
        parser.next_event(),
        Ok(Event::String(String::Borrowed("value")))
    );

    // Should fail with detailed error including line and column
    match parser.next_event() {
        Err(e) => {
            let err_msg = format!("{}", e);
            // Verify the error message includes line and column information
            assert!(
                err_msg.contains("line") && err_msg.contains("column"),
                "Error message should contain line and column info, got: {}",
                err_msg
            );
            // The error should be on line 3 where "invalid" appears
            assert!(
                err_msg.contains("line 3"),
                "Expected line 3, got: {}",
                err_msg
            );
        }
        other => panic!("Expected error for invalid JSON, got: {:?}", other),
    }
}

#[test]
fn test_multiline_error_tracking() {
    // Test error tracking across multiple lines
    let json = "[\n  1,\n  2,\n  true,\n]"; // Trailing comma at line 4, column 7 (comma position)
    let mut parser = SliceParser::new(json);

    // Parse through all events until we hit the error
    let err = loop {
        match parser.next_event() {
            Ok(Event::StartArray) | Ok(Event::Bool(_)) | Ok(Event::Number(_)) => continue,
            Err(e) => break e,
            other => panic!("Expected trailing comma error, got: {:?}", other),
        }
    };

    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("line 4"),
        "Expected line 4 in error message, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("column 7"),
        "Expected column 7 in error message, got: {}",
        err_msg
    );
}
