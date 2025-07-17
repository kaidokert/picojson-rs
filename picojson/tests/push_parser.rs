// SPDX-License-Identifier: Apache-2.0

use picojson::String;
use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

// A simplified, lifetime-free version of the Event for testing purposes.
#[derive(Debug, PartialEq, Eq)]
enum TestEvent<'a> {
    StartObject,
    EndObject,
    Bool(bool),
    EndDocument,
    Key(&'a str),
}

struct TestHandler<'a> {
    events: [Option<TestEvent<'a>>; 5],
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
            Event::Bool(b) => TestEvent::Bool(b),
            Event::EndDocument => TestEvent::EndDocument,
            Event::Key(String::Borrowed(k)) => TestEvent::Key(k),
            Event::Key(String::Unescaped(k)) => TestEvent::Key(k),
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
    let mut parser = PushParser::<_, DefaultConfig, _>::new(handler);
    parser.write(json, &mut scratch).unwrap();
    let handler = parser.destroy();

    let expected = [
        Some(TestEvent::StartObject),
        Some(TestEvent::Key("foo")),
        Some(TestEvent::Bool(true)),
        Some(TestEvent::EndObject),
        None,
    ];

    assert_eq!(&handler.events[..], &expected[..]);
}
