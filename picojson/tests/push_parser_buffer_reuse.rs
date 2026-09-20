// SPDX-License-Identifier: Apache-2.0

//! PushParser must accept chunks fed from a *reused* buffer — the shape of an
//! embedded network loop reading into a fixed receive buffer. Each `write`
//! call may only borrow the chunk for the duration of the call: any partial
//! token is copied to scratch before it returns.

use picojson::{DefaultConfig, Event, ParseError, PushParseError, PushParser, PushParserHandler};

#[derive(Debug, PartialEq)]
enum OwnedEvent {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    Key(String),
    String(String),
    Number(String),
    Bool(bool),
    Null,
    EndDocument,
}

struct Collector {
    events: Vec<OwnedEvent>,
}

impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for Collector {
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
        self.events.push(match event {
            Event::StartObject => OwnedEvent::StartObject,
            Event::EndObject => OwnedEvent::EndObject,
            Event::StartArray => OwnedEvent::StartArray,
            Event::EndArray => OwnedEvent::EndArray,
            Event::Key(k) => OwnedEvent::Key(k.as_str().to_string()),
            Event::String(s) => OwnedEvent::String(s.as_str().to_string()),
            Event::Number(n) => OwnedEvent::Number(n.as_str().to_string()),
            Event::Bool(b) => OwnedEvent::Bool(b),
            Event::Null => OwnedEvent::Null,
            Event::EndDocument => OwnedEvent::EndDocument,
        });
        Ok(())
    }
}

/// Feed `json` through a single fixed receive buffer that is overwritten
/// between `write` calls, `chunk_size` bytes at a time.
fn parse_via_reused_buffer(
    json: &[u8],
    chunk_size: usize,
) -> Result<Vec<OwnedEvent>, PushParseError<ParseError>> {
    let mut scratch = [0u8; 256];
    let mut parser =
        PushParser::<Collector, DefaultConfig>::new(Collector { events: Vec::new() }, &mut scratch);

    // The receive buffer: written over on every iteration, like a socket read.
    let mut recv = [0u8; 32];
    for chunk in json.chunks(chunk_size) {
        recv[..chunk.len()].copy_from_slice(chunk);
        // Scramble the rest so stale bytes would be caught if ever read.
        for byte in recv[chunk.len()..].iter_mut() {
            *byte = b'!';
        }
        parser.write::<ParseError>(&recv[..chunk.len()])?;
    }
    let collector = parser.finish::<ParseError>()?;
    Ok(collector.events)
}

/// Tokens deliberately straddle reuse boundaries: strings, `\u` escapes
/// (including a surrogate pair), multi-byte UTF-8, and numbers all get split
/// when fed in small chunks.
#[cfg(feature = "float")]
const JSON: &str = r#"{"name": "café 😀", "temp": -12.5e2, "list": [1, true, null], "end": "ok"}"#;
#[cfg(feature = "float")]
const TEMP: &str = "-12.5e2";

// Without `float`, -12.5e2 is an error under float-error and float-truncate,
// so use a multi-digit integer that fits every int width.
#[cfg(not(feature = "float"))]
const JSON: &str = r#"{"name": "café 😀", "temp": -125, "list": [1, true, null], "end": "ok"}"#;
#[cfg(not(feature = "float"))]
const TEMP: &str = "-125";

fn expected() -> Vec<OwnedEvent> {
    vec![
        OwnedEvent::StartObject,
        OwnedEvent::Key("name".into()),
        OwnedEvent::String("caf\u{e9} \u{1F600}".into()),
        OwnedEvent::Key("temp".into()),
        OwnedEvent::Number(TEMP.into()),
        OwnedEvent::Key("list".into()),
        OwnedEvent::StartArray,
        OwnedEvent::Number("1".into()),
        OwnedEvent::Bool(true),
        OwnedEvent::Null,
        OwnedEvent::EndArray,
        OwnedEvent::Key("end".into()),
        OwnedEvent::String("ok".into()),
        OwnedEvent::EndObject,
        OwnedEvent::EndDocument,
    ]
}

#[test]
fn reused_buffer_whole_chunks() {
    let events = parse_via_reused_buffer(JSON.as_bytes(), 32).unwrap();
    assert_eq!(events, expected());
}

#[test]
fn reused_buffer_small_chunks() {
    for chunk_size in [1, 2, 3, 5, 7, 13] {
        let events = parse_via_reused_buffer(JSON.as_bytes(), chunk_size).unwrap();
        assert_eq!(events, expected(), "chunk_size {chunk_size}");
    }
}
