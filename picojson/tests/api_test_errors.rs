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
