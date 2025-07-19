// SPDX-License-Identifier: Apache-2.0

use picojson::String;
use picojson::{DefaultConfig, Event, PushParser, PushParserHandler, PullParser};

// A simplified, lifetime-free version of the Event for testing purposes.
#[derive(Debug, PartialEq, Eq, Clone)]
enum TestEvent<'a> {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    Bool(bool),
    Null,
    EndDocument,
    Key(&'a str),
    String(&'a str),
}

struct TestHandler<'a> {
    events: [Option<TestEvent<'a>>; 10],
    idx: usize,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl<'a> TestHandler<'a> {
    fn new() -> Self {
        Self {
            events: core::array::from_fn(|_| None),
            idx: 0,
            _phantom: core::marker::PhantomData,
        }
    }
    fn add_event(&mut self, event: TestEvent<'a>) {
        if self.idx < self.events.len() {
            self.events[self.idx] = Some(event);
            self.idx += 1;
        }
    }
    
    /// Returns a slice of the events that have been filled
    fn events(&self) -> &[Option<TestEvent<'a>>] {
        &self.events[..self.idx]
    }
}

impl<'a, 'b> PushParserHandler<'a, 'b, ()> for TestHandler<'a>
where
    'b: 'a,
{
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ()> {
        let test_event = match event {
            Event::StartObject => TestEvent::StartObject,
            Event::EndObject => TestEvent::EndObject,
            Event::StartArray => TestEvent::StartArray,
            Event::EndArray => TestEvent::EndArray,
            Event::Bool(b) => TestEvent::Bool(b),
            Event::Null => TestEvent::Null,
            Event::EndDocument => TestEvent::EndDocument,
            Event::Key(String::Borrowed(k)) => TestEvent::Key(k),
            Event::Key(String::Unescaped(k)) => TestEvent::Key(k),
            Event::String(String::Borrowed(s)) => TestEvent::String(s),
            Event::String(String::Unescaped(s)) => TestEvent::String(s),
            _ => return Ok(()), // Ignore other events for now
        };
        self.add_event(test_event);
        Ok(())
    }
}

#[test_log::test]
fn test_string_with_actual_escapes() {
    // Create JSON with actual escape sequences (not escaped backslashes)
    // Use the EXACT same input as the SliceParser comparison test
    let json_string = r#"{"message": "Hello\nWorld\t!"}"#;
    let json = json_string.as_bytes();
    
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("message")),
        // The escape processing is working! We now get the actual unescaped content.
        // The debug logs show: "Buffer content as string: "Hello\n\tWorld!""
        // This demonstrates that escape sequences \\n and \\t are correctly processed.
        Some(TestEvent::String("Hello\nWorld\t!")),
        Some(TestEvent::EndObject),
        Some(TestEvent::EndDocument),
    ];

    println!("Actual events: {:?}", handler.events());
    assert_eq!(handler.events(), expected);
}

#[test]
fn test_debug_escape_events() {
    // Test with a simple quote escape to see if we get escape events
    let json_string = r#"{"test": "quote\"here"}"#;
    let json = json_string.as_bytes();
    
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    println!("Debug escape events: {:?}", handler.events());
    
    // For now, just check that it doesn't crash
    assert!(!handler.events().is_empty());
}

#[test_log::test]
fn test_slice_parser_comparison() {
    // Test the same JSON with SliceParser to see how it handles escapes
    let json_string = r#"{"message": "Hello\nWorld\t!"}"#;
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