// Test the new API entry points

use picojson::{Event, ParseError, PullParser, String};

#[test]
fn test_new_no_escapes() {
    let json = r#"{"name": "value", "number": 42, "bool": true}"#;
    let mut parser = PullParser::new(json);

    // Should parse successfully since there are no escapes
    // Events: StartObject, Key, String, Key, Number, Key, Bool, EndObject
    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("name")))
    );
    assert_eq!(
        parser.next_event(),
        Ok(Event::String(String::Borrowed("value")))
    );
    // Skip to end for brevity
    let mut remaining_count = 0;
    loop {
        match parser.next_event() {
            Ok(Event::EndDocument) => break,
            Ok(_) => remaining_count += 1,
            Err(e) => panic!("Parse error: {:?}", e),
        }
    }
    assert_eq!(remaining_count, 5); // Key, Number, Key, Bool, EndObject
}

#[test]
fn test_new_with_escapes_fails() {
    let json = r#"{"message": "Hello\nWorld"}"#; // Contains escape sequence
    let mut parser = PullParser::new(json);

    // Should parse until it hits the escape
    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("message")))
    );

    // Should fail on the escaped string
    match parser.next_event() {
        Err(ParseError::ScratchBufferFull) => {
            // Expected behavior
        }
        other => panic!("Expected ScratchBufferFull error, got: {:?}", other),
    }
}

#[test]
fn test_with_buffer_handles_escapes() {
    let json = r#"{"message": "Hello\nWorld"}"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    // Should parse successfully with escape handling
    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("message")))
    );

    // The escaped string should be unescaped
    match parser.next_event() {
        Ok(Event::String(String::Unescaped(s))) => {
            assert_eq!(s, "Hello\nWorld");
        }
        other => panic!("Expected unescaped string, got: {:?}", other),
    }

    assert_eq!(parser.next_event(), Ok(Event::EndObject));
    assert_eq!(parser.next_event(), Ok(Event::EndDocument));
}

#[test]
fn test_new_with_numbers_and_arrays() {
    #[cfg(feature = "float-error")]
    let json = r#"[1, 2, true, false, null]"#; // No floats for float-error config
    #[cfg(not(feature = "float-error"))]
    let json = r#"[1, 2.5, true, false, null]"#; // Include float for other configs

    let mut parser = PullParser::new(json);

    // Should handle all basic types without issues
    assert_eq!(parser.next_event(), Ok(Event::StartArray));
    assert!(matches!(parser.next_event(), Ok(Event::Number(_))));
    assert!(matches!(parser.next_event(), Ok(Event::Number(_))));
    assert_eq!(parser.next_event(), Ok(Event::Bool(true)));
    assert_eq!(parser.next_event(), Ok(Event::Bool(false)));
    assert_eq!(parser.next_event(), Ok(Event::Null));
    assert_eq!(parser.next_event(), Ok(Event::EndArray));
    assert_eq!(parser.next_event(), Ok(Event::EndDocument));
}

#[test]
fn test_mixed_string_types() {
    let json = r#"{"simple": "no_escapes", "complex": "with\tescapes"}"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    // Events: StartObject, Key("simple"), String("no_escapes"), Key("complex"), String("with\tescapes"), EndObject
    assert_eq!(parser.next_event(), Ok(Event::StartObject));
    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("simple")))
    );

    // First string should be borrowed (no escapes)
    match parser.next_event() {
        Ok(Event::String(String::Borrowed(s))) => {
            assert_eq!(s, "no_escapes");
        }
        other => panic!("Expected borrowed string, got: {:?}", other),
    }

    assert_eq!(
        parser.next_event(),
        Ok(Event::Key(String::Borrowed("complex")))
    );

    // Second string should be unescaped (has escapes)
    match parser.next_event() {
        Ok(Event::String(String::Unescaped(s))) => {
            assert_eq!(s, "with\tescapes");
        }
        other => panic!("Expected unescaped string, got: {:?}", other),
    }

    assert_eq!(parser.next_event(), Ok(Event::EndObject));
    assert_eq!(parser.next_event(), Ok(Event::EndDocument));
}
