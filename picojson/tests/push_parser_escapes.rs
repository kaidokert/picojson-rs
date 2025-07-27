// SPDX-License-Identifier: Apache-2.0

use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

/// Simple test handler that collects events as debug strings
struct EventCollector {
    events: Vec<String>,
}

impl EventCollector {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl<'a, 'b> PushParserHandler<'a, 'b, ()> for EventCollector {
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
        let event_desc = match event {
            Event::StartObject => "StartObject".to_string(),
            Event::EndObject => "EndObject".to_string(),
            Event::StartArray => "StartArray".to_string(),
            Event::EndArray => "EndArray".to_string(),
            Event::Bool(b) => format!("Bool({})", b),
            Event::Null => "Null".to_string(),
            Event::EndDocument => "EndDocument".to_string(),
            Event::Key(k) => format!("Key({})", k.as_ref()),
            Event::String(s) => format!("String({})", s.as_ref()),
            Event::Number(n) => format!("Number({})", n.as_str()),
        };
        self.events.push(event_desc);
        Ok(())
    }
}

#[test]
fn test_string_with_actual_escapes() {
    // Test that escape sequences in strings are properly processed
    let json_string = "{\"message\": \"Hello\\nWorld\\t!\"}";
    let json = json_string.as_bytes();

    let handler = EventCollector::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartObject".to_string(),
        "Key(message)".to_string(),
        // Escape sequences \\n and \\t should be converted to actual newline and tab
        "String(Hello\nWorld\t!)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}

#[test]
fn test_quote_escape() {
    // Test with a quote escape sequence
    let json_string = r#"{"test": "quote\"here"}"#;
    let json = json_string.as_bytes();

    let handler = EventCollector::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartObject".to_string(),
        "Key(test)".to_string(),
        // The \" should be converted to an actual quote character
        "String(quote\"here)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}

#[test]
fn test_escaped_key_with_newline() {
    // Test key with literal backslash-n characters (not escape sequence)
    let json_string = r#"{"ke\\ny": "value"}"#;
    let json = json_string.as_bytes();

    let handler = EventCollector::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);
    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartObject".to_string(),
        // This key contains literal backslash+n chars (not escape sequence) - correct behavior
        "Key(ke\\ny)".to_string(),
        "String(value)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}

#[test]
fn test_actual_key_escape_sequence() {
    // Test key with ACTUAL escape sequence: \n becomes newline character
    let json_string = r#"{"ke\ny": "value"}"#; // JSON with actual \n escape sequence
    let json = json_string.as_bytes();

    let handler = EventCollector::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);
    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartObject".to_string(),
        // Key escape processing should convert \n to actual newline
        "Key(ke\ny)".to_string(),
        "String(value)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}

#[test]
fn test_unicode_escapes() {
    // Test that Unicode escape sequences are properly decoded
    let json = br#"["\u0041\u0042\u0043"]"#;

    let mut buffer = [0u8; 64];
    let handler = EventCollector::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartArray".to_string(),
        "String(ABC)".to_string(), // \u0041\u0042\u0043 should decode to ABC
        "EndArray".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}

#[test]
fn test_escaped_key_with_quote() {
    // Test key with quote escape - key "quo\"te" with value "data"
    let json_string = r#"{"quo\"te": "data"}"#;
    let json = json_string.as_bytes();

    let handler = EventCollector::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);
    parser.write(json).unwrap();
    parser.finish::<()>().unwrap();
    let handler = parser.destroy();

    let expected = vec![
        "StartObject".to_string(),
        // Key with quote escape should be processed correctly
        "Key(quo\"te)".to_string(),
        "String(data)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected);
}
