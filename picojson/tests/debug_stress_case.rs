// SPDX-License-Identifier: Apache-2.0

use picojson::{ChunkReader, Event, PullParser, StreamParser};

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
