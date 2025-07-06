// SPDX-License-Identifier: Apache-2.0

use picojson::{Event, PullParser, Reader, StreamParser};

use test_env_log::test;

struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Reader for SliceReader<'a> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }

        let to_copy = remaining.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

#[test]
fn test_key_value_pair_with_20_byte_buffer() {
    let json = b"{ \"hello\" : \"world\" }";
    let reader = SliceReader::new(json);
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
