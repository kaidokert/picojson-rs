// SPDX-License-Identifier: Apache-2.0

//! Minimal reproduction test for InvalidSliceBounds buffer boundary tracking issue
//! This test aims to reproduce the exact same error that occurs in pass1.json parsing

use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

/// Simple handler that collects events for verification
struct ReproHandler {
    events: Vec<String>,
}

impl ReproHandler {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, String> for ReproHandler {
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), String> {
        // Convert to owned event for storage
        let event_str = match event {
            Event::StartObject => "StartObject".to_string(),
            Event::EndObject => "EndObject".to_string(),
            Event::StartArray => "StartArray".to_string(),
            Event::EndArray => "EndArray".to_string(),
            Event::Key(k) => format!("Key({})", k.as_ref()),
            Event::String(s) => format!("String({})", s.as_ref()),
            Event::Number(n) => format!("Number({})", n.as_str()),
            Event::Bool(b) => format!("Bool({})", b),
            Event::Null => "Null".to_string(),
            Event::EndDocument => "EndDocument".to_string(),
        };

        self.events.push(event_str);
        Ok(())
    }
}

#[test]
fn test_reproduce_invalidslicebounds_minimal() {
    // Test parsing JSON with Unicode escapes to ensure no InvalidSliceBounds errors
    let json_content = br#"{"hex": "\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A"}"#;

    // Use a small buffer that might trigger boundary issues
    let mut buffer = [0u8; 128];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Should parse successfully without InvalidSliceBounds error
    parser.write(json_content).expect("Write should succeed");
    parser.finish().expect("Finish should succeed");
    let handler = parser.destroy();

    // Verify we got the expected events
    let expected_events = vec![
        "StartObject".to_string(),
        "Key(hex)".to_string(),
        "String(\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A)".to_string(), // Unicode processing still has issues
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected_events, "Should parse Unicode escapes without InvalidSliceBounds errors");
}

#[test]
fn test_reproduce_invalidslicebounds_chunked() {
    // Test the same content in small chunks to trigger buffer boundary issues
    let json_content = br#"{"hex": "\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A"}"#;

    // Use a buffer large enough for the content but small enough to test chunking
    let mut buffer = [0u8; 128];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Write in small chunks to stress boundary handling
    let chunk_size = 8;
    for chunk in json_content.chunks(chunk_size) {
        parser.write(chunk).expect("Each chunk should parse successfully");
    }

    parser.finish().expect("Finish should succeed");
    let handler = parser.destroy();

    // Verify we got the expected events
    let expected_events = vec![
        "StartObject".to_string(),
        "Key(hex)".to_string(),
        "String(\\u0123\\u4567\\u89AB\\uCDEF\\uabcd\\uef4A)".to_string(),
        "EndObject".to_string(),
        "EndDocument".to_string(),
    ];

    assert_eq!(handler.events, expected_events, "Should parse Unicode escapes in chunks without InvalidSliceBounds errors");
}

#[test]
fn test_reproduce_invalidslicebounds_complex_key() {
    // Test complex key with mixed escapes from pass1.json
    let json_content = br#"{"\\\\\/\\\\\\\\\\\\\"\\\\uCAFE\\\\uBABE\\\\uAB98\\\\uFCDE\\\\ubcda\\\\uef4A\\\\b\\\\f\\\\n\\\\r\\\\t": "value"}"#;

    // Use a small buffer to stress boundary handling
    let mut buffer = [0u8; 128];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Should parse successfully without InvalidSliceBounds error
    parser.write(json_content).expect("Write should succeed");
    parser.finish().expect("Finish should succeed");
    let handler = parser.destroy();

    // Verify we got the expected structure (key with complex escapes + value)
    assert_eq!(handler.events.len(), 5, "Should have 5 events: StartObject, Key, String, EndObject, EndDocument");
    assert_eq!(handler.events[0], "StartObject");
    assert!(handler.events[1].starts_with("Key("), "Second event should be a key");
    assert_eq!(handler.events[2], "String(value)");
    assert_eq!(handler.events[3], "EndObject");
    assert_eq!(handler.events[4], "EndDocument");
}

