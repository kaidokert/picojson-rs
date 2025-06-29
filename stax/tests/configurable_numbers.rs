// Comprehensive tests for configurable number handling
// These tests demonstrate the various compilation configurations

use stax::{Event, NumberResult, PullParser};

#[test]
#[cfg(feature = "int32")]
fn test_int32_overflow() {
    let input = r#"{"value": 9999999999}"#; // Larger than i32::MAX (2,147,483,647)
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "9999999999");
            assert!(matches!(num.parsed(), NumberResult::IntegerOverflow));
            assert_eq!(num.as_int(), None); // Too large for i32
        }
        other => panic!("Expected Number, got: {:?}", other),
    }
}

#[test]
#[cfg(feature = "int64")]
fn test_int64_handles_large_numbers() {
    let input = r#"{"value": 9999999999}"#; // Within i64 range
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "9999999999");
            assert!(matches!(num.parsed(), NumberResult::Integer(9999999999)));
            assert_eq!(num.as_int(), Some(9999999999));
        }
        other => panic!("Expected Number, got: {:?}", other),
    }
}

#[test]
#[cfg(all(not(feature = "float"), feature = "float-error"))]
fn test_float_error_behavior() {
    let input = r#"{"value": 3.14}"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    // Should parse normally until we hit the float
    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    // Float should cause an error
    match parser.next_event() {
        Err(stax::ParseError::FloatNotAllowed) => {
            // Expected behavior - test passes
        }
        other => panic!("Expected FloatNotAllowed error, got: {:?}", other),
    }
}

#[test]
#[cfg(all(not(feature = "float"), feature = "float-truncate", feature = "int32"))]
fn test_float_truncate_to_i32() {
    let input = r#"[1.7, 2.9, 3.1]"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartArray)));

    // 1.7 -> 1
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "1.7");
            assert!(matches!(num.parsed(), NumberResult::FloatTruncated(1)));
            assert_eq!(num.as_int(), Some(1));
        }
        other => panic!("Expected truncated Number(1), got: {:?}", other),
    }

    // 2.9 -> 2
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "2.9");
            assert!(matches!(num.parsed(), NumberResult::FloatTruncated(2)));
            assert_eq!(num.as_int(), Some(2));
        }
        other => panic!("Expected truncated Number(2), got: {:?}", other),
    }

    // 3.1 -> 3
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "3.1");
            assert!(matches!(num.parsed(), NumberResult::FloatTruncated(3)));
            assert_eq!(num.as_int(), Some(3));
        }
        other => panic!("Expected truncated Number(3), got: {:?}", other),
    }

    assert!(matches!(parser.next_event(), Ok(Event::EndArray)));
}

#[test]
#[cfg(all(
    not(feature = "float"),
    feature = "float-truncate",
    not(feature = "int32")
))]
fn test_float_truncate_to_i64() {
    let input = r#"[1.7, 2.9, 3.1]"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartArray)));

    // Should truncate to i64 values
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "1.7");
            assert!(matches!(num.parsed(), NumberResult::FloatTruncated(1i64)));
        }
        other => panic!("Expected truncated Number, got: {:?}", other),
    }
}

#[test]
#[cfg(all(not(feature = "float"), feature = "float-truncate"))]
fn test_float_truncate_scientific_notation() {
    let input = r#"{"value": 1.5e2}"#; // Scientific notation should error in truncate mode
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    // Scientific notation should cause InvalidNumber error to avoid float math
    match parser.next_event() {
        Err(stax::ParseError::InvalidNumber) => {
            // Expected behavior - test passes
        }
        other => panic!(
            "Expected InvalidNumber error for scientific notation, got: {:?}",
            other
        ),
    }
}

#[test]
#[cfg(all(
    not(feature = "float"),
    feature = "int64",
    not(any(
        feature = "float-error",
        feature = "float-skip",
        feature = "float-truncate"
    ))
))]
fn test_default_float_disabled_behavior() {
    let input = r#"{"value": 3.14}"#;
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "3.14");
            assert!(matches!(num.parsed(), NumberResult::FloatDisabled));
            assert_eq!(num.as_int(), None);

            // Raw string should still be available for manual parsing
            assert_eq!(num.as_str(), "3.14");
            let manual_parse: Result<f64, _> = num.parse();
            assert!(manual_parse.is_ok());
        }
        other => panic!("Expected Number with FloatDisabled, got: {:?}", other),
    }
}

#[test]
#[cfg(feature = "int32")]
fn test_mixed_numbers_with_i32() {
    let input = r#"{"small": 42, "large": 999999999999}"#; // large > i32::MAX
    let mut scratch = [0u8; 1024];
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    // Small number should parse fine
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "42");
            assert!(matches!(num.parsed(), NumberResult::Integer(42)));
            assert_eq!(num.as_int(), Some(42));
        }
        other => panic!("Expected Number(42), got: {:?}", other),
    }

    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    // Large number should overflow
    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "999999999999");
            assert!(matches!(num.parsed(), NumberResult::IntegerOverflow));
            assert_eq!(num.as_int(), None);

            // But raw string is still available
            assert_eq!(num.as_str(), "999999999999");
        }
        other => panic!("Expected Number with overflow, got: {:?}", other),
    }
}

// This test ensures the library compiles and works with the most restrictive embedded configuration
#[test]
#[cfg(all(feature = "int32", not(feature = "float"), feature = "float-error"))]
fn test_embedded_friendly_config() {
    // This configuration uses:
    // - i32 integers (no 64-bit math)
    // - No float support
    // - Error on floats (fail fast)

    let input = r#"{"sensor": 42, "status": 1}"#;
    let mut scratch = [0u8; 256]; // Small buffer for embedded
    let mut parser = PullParser::new_with_buffer(input, &mut scratch);

    // Should parse integers normally
    assert!(matches!(parser.next_event(), Ok(Event::StartObject)));
    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));

    match parser.next_event() {
        Ok(Event::Number(num)) => {
            assert_eq!(num.as_str(), "42");
            assert!(matches!(num.parsed(), NumberResult::Integer(42i32)));
            assert_eq!(num.as_int(), Some(42i32));
        }
        other => panic!("Expected Number(42), got: {:?}", other),
    }

    assert!(matches!(parser.next_event(), Ok(Event::Key(_))));
    assert!(matches!(parser.next_event(), Ok(Event::Number(_))));
    assert!(matches!(parser.next_event(), Ok(Event::EndObject)));
}
