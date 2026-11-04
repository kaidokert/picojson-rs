// SPDX-License-Identifier: Apache-2.0

//! A number starting at document position 0 keeps its first byte in every
//! parser. Extraction that starts one byte too far forward drops a digit here.

use picojson::{ChunkReader, Event, PullParser, SliceParser, StreamParser};

const CASES: &[(&str, &str)] = &[("123 ", "123"), ("123", "123"), ("7", "7"), ("-5 ", "-5")];

fn number(event: Option<Result<Event<'_, '_>, picojson::ParseError>>) -> String {
    match event {
        Some(Ok(Event::Number(n))) => n.as_str().to_string(),
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn slice_parser() {
    for (json, expected) in CASES {
        let mut parser = SliceParser::new(json);
        assert_eq!(number(parser.next()), *expected, "{json:?}");
        assert!(parser.next().is_none(), "{json:?}");
    }
}

#[test]
fn stream_parser_across_refills() {
    for (json, expected) in CASES {
        for (buffer_len, chunk_len) in [(5, 1), (8, 3), (64, 64)] {
            let mut buffer = vec![0u8; buffer_len];
            let reader = ChunkReader::new(json.as_bytes(), chunk_len);
            let mut parser = StreamParser::new(reader, &mut buffer);
            let context = format!("{json:?} buffer={buffer_len} chunk={chunk_len}");
            assert_eq!(number(parser.next()), *expected, "{context}");
            assert!(parser.next().is_none(), "{context}");
        }
    }
}
