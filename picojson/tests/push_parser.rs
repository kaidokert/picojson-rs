// SPDX-License-Identifier: Apache-2.0

use picojson::String;
use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

// A simplified, lifetime-free version of the Event for testing purposes.
#[derive(Debug, PartialEq, Eq)]
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

#[test]
fn test_simple_object() {
    let json = br#"{ "foo": true }"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("foo")),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::EndObject),
        Some(TestEvent::EndDocument),
        None,
        None,
        None,
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}

#[test]
fn test_array_with_primitives() {
    let json = br#"[true, false, null]"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartArray),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::Bool(false)),
        Some(TestEvent::Null),
        Some(TestEvent::EndArray),
        Some(TestEvent::EndDocument),
        None,
        None,
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}

#[test]
fn test_nested_structure() {
    let json = br#"{"items": [true, false]}"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("items")),
        Some(TestEvent::StartArray),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::Bool(false)),
        Some(TestEvent::EndArray),
        Some(TestEvent::EndObject),
        Some(TestEvent::EndDocument),
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}

#[test]
fn test_object_with_string_value() {
    let json = br#"{"name": "picojson", "active": true}"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("name")),
        Some(TestEvent::String("picojson")),
        Some(TestEvent::Key("active")),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::EndObject),
        Some(TestEvent::EndDocument),
        None,
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}

#[test]
fn test_array_with_strings() {
    let json = br#"["hello", "world", true]"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartArray),
        Some(TestEvent::String("hello")),
        Some(TestEvent::String("world")),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::EndArray),
        Some(TestEvent::EndDocument),
        None,
        None,
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}

#[test]
fn test_string_with_simple_escapes() {
    let json = br#"{"message": "Hello\nWorld\t!"}"#;
    let mut scratch = [0u8; 128];
    let handler = TestHandler::new();
    let mut buffer = [0u8; 256];
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler, &mut buffer);
    parser.write(json, &mut scratch).unwrap();
    parser.finish().unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("message")),
        // For now, escape processing is not fully implemented in Phase 2
        // So the string will be parsed as-is without processing escapes
        Some(TestEvent::String("Hello\\nWorld\\t!")),
        Some(TestEvent::EndObject),
        Some(TestEvent::EndDocument),
        None,
        None,
        None,
        None,
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}
