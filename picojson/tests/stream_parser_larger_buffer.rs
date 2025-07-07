// SPDX-License-Identifier: Apache-2.0

use picojson::{ChunkReader, Event, PullParser, StreamParser};

#[test]
fn test_key_value_pair_with_10_byte_buffer() {
    let json = b"{ \"hello\" : \"world\" }";
    let reader = ChunkReader::full_slice(json);
    let mut buffer = [0u8; 10];
    let mut parser = StreamParser::new(reader, &mut buffer);

    let expected_events = [
        Event::StartObject,
        Event::Key("hello".into()),
        Event::String("world".into()),
        Event::EndObject,
    ];

    for (i, expected_event) in expected_events.iter().enumerate() {
        match parser.next_event() {
            Ok(event) => assert_eq!(&event, expected_event, "Mismatch at event index {}", i),
            Err(e) => panic!("Parser error at event index {}: {:?}", i, e),
        }
    }

    assert_eq!(
        parser.next_event().unwrap(),
        Event::EndDocument,
        "Expected EndDocument after all other events"
    );
}
