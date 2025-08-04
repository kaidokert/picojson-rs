// SPDX-License-Identifier: Apache-2.0

//! Minimal reproduction test for InvalidSliceBounds buffer boundary tracking issue
//! This test aims to reproduce the exact same error that occurs in pass1.json parsing

use picojson::{DefaultConfig, Event, ParseError, PushParser, PushParserHandler};

/// Handler that compares events immediately as they arrive for detailed validation
struct ReproHandler<'expected> {
    expected_events: &'expected [&'expected str],
    current_index: usize,
}

impl<'expected> ReproHandler<'expected> {
    fn new(expected_events: &'expected [&'expected str]) -> Self {
        Self {
            expected_events,
            current_index: 0,
        }
    }

    fn assert_complete(&self) {
        assert_eq!(
            self.current_index,
            self.expected_events.len(),
            "Expected {} events, but only received {}",
            self.expected_events.len(),
            self.current_index
        );
    }

    fn assert_event_matches(&mut self, received: &Event) {
        assert!(
            self.current_index < self.expected_events.len(),
            "Received more events than expected. Got event at index {} but only expected {} events total",
            self.current_index,
            self.expected_events.len()
        );

        let expected_str = self.expected_events[self.current_index];
        let received_str = self.event_to_string(received);

        assert_eq!(
            expected_str, received_str,
            "Event mismatch at index {}",
            self.current_index
        );

        self.current_index += 1;
    }

    fn event_to_string(&self, event: &Event) -> String {
        match event {
            Event::StartObject => "StartObject".to_string(),
            Event::EndObject => "EndObject".to_string(),
            Event::StartArray => "StartArray".to_string(),
            Event::EndArray => "EndArray".to_string(),
            Event::Key(k) => format!("Key({})", k.as_str()),
            Event::String(s) => format!("String({})", s.as_str()),
            Event::Number(n) => format!("Number({})", n.as_str()),
            Event::Bool(b) => format!("Bool({})", b),
            Event::Null => "Null".to_string(),
            Event::EndDocument => "EndDocument".to_string(),
        }
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for ReproHandler<'_> {
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
        self.assert_event_matches(&event);
        Ok(())
    }
}

#[test]
fn test_reproduce_invalidslicebounds_minimal() {
    // Test parsing JSON with Unicode escapes to ensure no InvalidSliceBounds errors
    let json_content = br#"{"hex": "\u0123\u4567\u89AB\uCDEF\uabcd\uef4A"}"#;

    // Use a small buffer that might trigger boundary issues
    let mut buffer = [0u8; 128];

    // Define expected events with properly decoded Unicode escapes
    let expected_events = [
        "StartObject",
        "Key(hex)",
        "String(ģ䕧覫췯ꯍ\u{ef4a})", // Unicode escapes properly decoded to characters
        "EndObject",
        "EndDocument",
    ];

    let handler = ReproHandler::new(&expected_events);
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Should parse successfully without InvalidSliceBounds error
    parser.write(json_content).expect("Write should succeed");
    let handler = parser
        .finish::<ParseError>()
        .expect("Finish should succeed");

    // Verify all expected events were received
    handler.assert_complete();
}

#[test]
fn test_reproduce_invalidslicebounds_chunked() {
    // Test the same content in small chunks to trigger buffer boundary issues
    let json_content = br#"{"hex": "\u0123\u4567\u89AB\uCDEF\uabcd\uef4A"}"#;

    // Use a buffer large enough for the content but small enough to test chunking
    let mut buffer = [0u8; 128];

    // Define expected events (same as previous test)
    let expected_events = [
        "StartObject",
        "Key(hex)",
        "String(ģ䕧覫췯ꯍ\u{ef4a})",
        "EndObject",
        "EndDocument",
    ];

    let handler = ReproHandler::new(&expected_events);
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Write in small chunks to stress boundary handling
    let chunk_size = 8;
    for chunk in json_content.chunks(chunk_size) {
        parser
            .write(chunk)
            .expect("Each chunk should parse successfully");
    }

    let handler = parser
        .finish::<ParseError>()
        .expect("Finish should succeed");

    // Verify all expected events were received
    handler.assert_complete();
}

#[test]
fn test_reproduce_invalidslicebounds_complex_key() {
    // Test complex key with mixed escapes from pass1.json
    let json_content = br#"{"\\\/\\\\\"\uCAFE\uBABE\uAB98\uFCDE\ubcda\uef4A\b\f\n\r\t": "value"}"#;

    // Use a small buffer to stress boundary handling
    let mut buffer = [0u8; 128];

    // Define expected events with complex key containing decoded escape sequences
    let expected_events = [
        "StartObject",
        "Key(\\/\\\\\"쫾몾ꮘﳞ볚\u{ef4a}\u{8}\u{c}\n\r\t)", // Complex key with decoded escapes
        "String(value)",
        "EndObject",
        "EndDocument",
    ];

    let handler = ReproHandler::new(&expected_events);
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Should parse successfully without InvalidSliceBounds error
    parser.write(json_content).expect("Write should succeed");
    let handler = parser
        .finish::<ParseError>()
        .expect("Finish should succeed");

    // Verify all expected events were received with proper escape sequence decoding
    handler.assert_complete();
}
