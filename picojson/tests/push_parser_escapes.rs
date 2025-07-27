// SPDX-License-Identifier: Apache-2.0

use picojson::{DefaultConfig, Event, PullParser, PushParser, PushParserHandler};

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
    let json_string = r#"{"message": "Hello\\nWorld\\t!"}"#;
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
        // TODO: The escape sequences \\n and \\t should be converted to actual newline and tab (Issue #3)
        // Currently in this test context, they remain as literal sequences
        "String(Hello\\nWorld\\t!)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    println!("Actual events: {:?}", handler.events);
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

    println!("Quote escape events: {:?}", handler.events);
    assert_eq!(handler.events, expected);
}

#[test]
fn test_slice_parser_comparison() {
    // Test the same JSON with SliceParser to see how it handles escapes
    let json_string = r#"{"message": "Hello\\nWorld\\t!"}"#;
    let mut scratch = [0u8; 256];
    let mut parser = picojson::SliceParser::with_buffer(json_string, &mut scratch);

    println!("SliceParser results:");
    while let Ok(event) = parser.next_event() {
        match event {
            picojson::Event::EndDocument => break,
            _ => println!("  {:?}", event),
        }
    }
}

#[test]
fn test_escaped_key_with_newline() {
    // Test key with escape sequence - key "ke\ny" with value "value"
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
        // TODO: Key with escape sequence should be processed correctly (Issue #3)
        // Currently in this test context, they remain as literal sequences
        "Key(ke\\ny)".to_string(),
        "String(value)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    println!("Escaped key test - actual events: {:?}", handler.events);
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

    println!(
        "Escaped key quote test - actual events: {:?}",
        handler.events
    );
    assert_eq!(handler.events, expected);
}
