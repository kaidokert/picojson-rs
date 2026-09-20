// SPDX-License-Identifier: Apache-2.0

//! PushParser throughput measurement over a JSON file.
//!
//! Usage: throughput <file.json> [iterations] [chunk_size]
//!
//! Feeds the file through `PushParser` with a no-op handler `iterations`
//! times (default 10), in `chunk_size`-byte writes (default: whole file),
//! and prints MB/s. The scratch buffer is 64 KiB; bump it if your input
//! carries single tokens larger than that.

use picojson::{DefaultConfig, Event, ParseError, PushParser, PushParserHandler};

struct Nop {
    events: u64,
}

impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for Nop {
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), ParseError> {
        // Touch the event so content extraction cannot be optimized away.
        self.events += match event {
            Event::Key(s) | Event::String(s) => (!s.as_str().is_empty()) as u64,
            Event::Number(n) => (!n.as_str().is_empty()) as u64,
            _ => 1,
        };
        Ok(())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: throughput <file.json> [iters] [chunk]");
    let iters: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let chunk: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let data = std::fs::read(&path).expect("read input file");
    let mut scratch = vec![0u8; 64 * 1024];
    let mut total_events = 0u64;

    let t0 = std::time::Instant::now();
    for _ in 0..iters {
        let mut parser: PushParser<Nop, DefaultConfig> =
            PushParser::new(Nop { events: 0 }, &mut scratch);
        for piece in data.chunks(chunk.max(1)) {
            parser.write::<ParseError>(piece).expect("parse");
        }
        let nop = parser.finish::<ParseError>().expect("finish");
        total_events += nop.events;
    }
    let elapsed = t0.elapsed();

    let mb = (data.len() as f64 * iters as f64) / 1e6;
    println!(
        "{} bytes x {} iters ({} chunks): {:.3}s -> {:.1} MB/s ({} events)",
        data.len(),
        iters,
        if chunk == usize::MAX {
            "whole".to_string()
        } else {
            chunk.to_string()
        },
        elapsed.as_secs_f64(),
        mb / elapsed.as_secs_f64(),
        total_events
    );
}
