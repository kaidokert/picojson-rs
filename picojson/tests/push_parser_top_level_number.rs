// SPDX-License-Identifier: Apache-2.0

//! Top-level numbers in PushParser: a number starting at document position 0
//! keeps its first digit however the input is chunked, and a stray container
//! close after it reaches the handler as an error only, never as events.

use picojson::{DefaultConfig, Event, ParseError, PushParseError, PushParser, PushParserHandler};

struct Collector {
    events: Vec<String>,
}

impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for &mut Collector {
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
        self.events.push(match event {
            Event::Number(n) => format!("Number({})", n.as_str()),
            other => format!("{other:?}"),
        });
        Ok(())
    }
}

/// Events delivered to the handler, plus the first `write` error if any.
fn parse(json: &[u8], chunk_size: usize) -> (Vec<String>, Option<PushParseError<ParseError>>) {
    let mut collector = Collector { events: Vec::new() };
    let mut scratch = [0u8; 64];
    let mut parser = PushParser::<_, DefaultConfig>::new(&mut collector, &mut scratch);
    let mut error = None;
    for chunk in json.chunks(chunk_size) {
        if let Err(e) = parser.write::<ParseError>(chunk) {
            error = Some(e);
            break;
        }
    }
    if error.is_none() {
        error = parser.finish::<ParseError>().err();
    }
    (collector.events, error)
}

#[test]
fn number_at_document_start_keeps_first_digit() {
    for chunk_size in [1, 2, 3, 4, 64] {
        let (events, error) = parse(b"123 ", chunk_size);
        assert_eq!(error, None, "chunk_size {chunk_size}");
        assert_eq!(
            events,
            ["Number(123)", "EndDocument"],
            "chunk_size {chunk_size}"
        );
    }
}

#[test]
fn single_digit_at_document_start() {
    for chunk_size in [1, 2] {
        let (events, error) = parse(b"7 ", chunk_size);
        assert_eq!(error, None, "chunk_size {chunk_size}");
        assert_eq!(
            events,
            ["Number(7)", "EndDocument"],
            "chunk_size {chunk_size}"
        );
    }
}

#[test]
fn stray_close_after_top_level_number_delivers_no_events() {
    for json in [b"1]", b"1}"] {
        for chunk_size in [1, 2] {
            let (events, error) = parse(json, chunk_size);
            assert!(
                matches!(error, Some(PushParseError::Parse(_))),
                "{json:?} chunk_size {chunk_size}: {error:?}"
            );
            assert_eq!(
                events,
                Vec::<String>::new(),
                "{json:?} chunk_size {chunk_size}"
            );
        }
    }
}
