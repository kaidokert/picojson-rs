// Integration test for JsonNumber ergonomic APIs
use picojson::{Event, PullParser};

#[test]
fn test_json_number_display_trait() {
    let json = r#"{"int": 42, "big": 12345678901234567890, "float": 3.25}"#;
    let mut scratch = [0u8; 128];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    // Skip to first number
    assert_eq!(parser.next_event().unwrap(), Event::StartObject);
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));

    // Test Display trait on integer
    if let Event::Number(num) = parser.next_event().unwrap() {
        let displayed = format!("{}", num);
        assert_eq!(displayed, "42");
    } else {
        panic!("Expected Number event");
    }

    // Skip to big number
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));

    // Test Display trait on overflow
    if let Event::Number(num) = parser.next_event().unwrap() {
        let displayed = format!("{}", num);
        assert_eq!(displayed, "12345678901234567890"); // Should show raw string for overflow
    } else {
        panic!("Expected Number event");
    }

    // Skip to float
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));

    // Test Display trait on float (configuration dependent)
    #[cfg(feature = "float-error")]
    {
        // float-error should return an error when encountering floats
        let result = parser.next_event();
        assert!(
            result.is_err(),
            "Expected error for float with float-error configuration"
        );
    }
    #[cfg(not(feature = "float-error"))]
    {
        if let Event::Number(num) = parser.next_event().unwrap() {
            let displayed = format!("{}", num);
            #[cfg(feature = "float")]
            assert_eq!(displayed, "3.25");
            #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
            assert_eq!(displayed, "3"); // Float truncated to integer
            #[cfg(all(not(feature = "float"), not(feature = "float-truncate")))]
            assert_eq!(displayed, "3.25"); // Raw string when float disabled
        } else {
            panic!("Expected Number event");
        }
    }
}

#[test]
fn test_json_number_deref_trait() {
    let json = r#"[123, -456]"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartArray);

    // Test Deref trait (enables &*num syntax)
    if let Event::Number(num) = parser.next_event().unwrap() {
        let s: &str = &*num; // This uses Deref
        assert_eq!(s, "123");

        // Also test direct dereference
        assert_eq!(*num, *"123");
    } else {
        panic!("Expected Number event");
    }

    // Test negative number
    if let Event::Number(num) = parser.next_event().unwrap() {
        let s: &str = &*num;
        assert_eq!(s, "-456");
    } else {
        panic!("Expected Number event");
    }
}

#[test]
fn test_json_number_as_ref_trait() {
    let json = r#"{"zero": 0, "negative": -999}"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartObject);
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));

    // Test AsRef<str> trait
    if let Event::Number(num) = parser.next_event().unwrap() {
        let s: &str = num.as_ref(); // This uses AsRef<str>
        assert_eq!(s, "0");

        // Function that takes AsRef<str>
        fn takes_str_ref<T: AsRef<str>>(value: T) -> String {
            value.as_ref().to_uppercase()
        }
        assert_eq!(takes_str_ref(num), "0");
    } else {
        panic!("Expected Number event");
    }

    // Test with negative number
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
    if let Event::Number(num) = parser.next_event().unwrap() {
        let s: &str = num.as_ref();
        assert_eq!(s, "-999");

        // Test AsRef works with generic functions
        fn string_length<T: AsRef<str>>(value: T) -> usize {
            value.as_ref().len()
        }
        assert_eq!(string_length(num), 4); // "-999".len()
    } else {
        panic!("Expected Number event");
    }
}

#[test]
fn test_json_number_parse_method() {
    let json = r#"{"value": 42}"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartObject);
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));

    if let Event::Number(num) = parser.next_event().unwrap() {
        // Test parse method with different types
        let as_u32: u32 = num.parse().unwrap();
        assert_eq!(as_u32, 42);

        let as_i64: i64 = num.parse().unwrap();
        assert_eq!(as_i64, 42);

        let as_f64: f64 = num.parse().unwrap();
        assert_eq!(as_f64, 42.0);

        // Test that it's using the string representation
        assert_eq!(num.as_str(), "42");
    } else {
        panic!("Expected Number event");
    }
}

#[test]
fn test_json_number_type_checking_methods() {
    let json = r#"[42, 3.25]"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartArray);

    // Test integer type checking
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert!(num.is_integer());
        assert!(!num.is_float());
    } else {
        panic!("Expected Number event");
    }

    // Test float type checking (configuration dependent)
    #[cfg(feature = "float-error")]
    {
        // float-error should return an error when encountering floats
        let result = parser.next_event();
        assert!(
            result.is_err(),
            "Expected error for float with float-error configuration"
        );
    }
    #[cfg(not(feature = "float-error"))]
    {
        if let Event::Number(num) = parser.next_event().unwrap() {
            assert!(!num.is_integer());
            assert!(num.is_float());
        } else {
            panic!("Expected Number event");
        }
    }
}

#[test]
#[cfg(feature = "float")]
fn test_json_number_as_f64_method() {
    let json = r#"[42, 3.25, 1e10]"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartArray);

    // Integer converted to f64
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_f64(), Some(42.0));
    } else {
        panic!("Expected Number event");
    }

    // Float value
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_f64(), Some(3.25));
    } else {
        panic!("Expected Number event");
    }

    // Scientific notation
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_f64(), Some(1e10));
    } else {
        panic!("Expected Number event");
    }
}

#[test]
fn test_json_number_as_int_method() {
    let json = r#"[42, -123, 12345678901234567890]"#;
    let mut scratch = [0u8; 64];
    let mut parser = PullParser::with_buffer(json, &mut scratch);

    assert_eq!(parser.next_event().unwrap(), Event::StartArray);

    // Regular integer
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_int(), Some(42));
    } else {
        panic!("Expected Number event");
    }

    // Negative integer
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_int(), Some(-123));
    } else {
        panic!("Expected Number event");
    }

    // Overflow integer
    if let Event::Number(num) = parser.next_event().unwrap() {
        assert_eq!(num.as_int(), None); // Should be None due to overflow
                                        // But string representation still available
        assert_eq!(num.as_str(), "12345678901234567890");
    } else {
        panic!("Expected Number event");
    }
}
