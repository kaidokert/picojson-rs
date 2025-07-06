// SPDX-License-Identifier: Apache-2.0

use picojson::{Event, PullParser, Reader, StreamParser};

use test_log::test;

/// Configurable reader that provides data in specified chunk sizes
struct ChunkReader<'a> {
    data: &'a [u8],
    pos: usize,
    chunk_size: usize,
}

impl<'a> ChunkReader<'a> {
    fn new(data: &'a [u8], chunk_size: usize) -> Self {
        Self {
            data,
            pos: 0,
            chunk_size: chunk_size.max(1),
        }
    }
}

impl<'a> Reader for ChunkReader<'a> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }

        let to_copy = remaining.min(buf.len()).min(self.chunk_size);
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

#[test]
fn debug_specific_failure_case() {
    // Test the specific failing case: buffer=20, chunk=1
    let json = br#"{"hello": "world", "count": 42}"#;
    println!("JSON: {:?}", std::str::from_utf8(json).unwrap());
    println!("JSON length: {} bytes", json.len());

    let reader = ChunkReader::new(json, 1); // 1-byte chunks
    let mut buffer = vec![0u8; 20]; // 20-byte buffer
    let mut parser = StreamParser::new(reader, &mut buffer);

    println!("\nParsing events:");
    for i in 0..10 {
        match parser.next_event() {
            Ok(event) => {
                println!("Event {}: {:?}", i, event);
                if matches!(event, Event::EndDocument) {
                    break;
                }
            }
            Err(e) => {
                println!("Error at event {}: {:?}", i, e);
                break;
            }
        }
    }
}
