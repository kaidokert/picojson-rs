// Example demonstrating StreamParser with a Reader over a fixed-size array

use picojson::{ChunkReader, Event, ParseError, PullParser, StreamParser};

fn main() -> Result<(), ParseError> {
    println!("🚀 StreamParser Demo with ChunkReader");
    println!("=====================================");

    // Test JSON with various data types including escape sequences
    let json = br#"{"name": "hello\nworld", "items": [1, 2.5, true, null], "count": 42}"#;

    println!("📄 Input JSON: {}", std::str::from_utf8(json).unwrap());
    println!("📏 Total size: {} bytes", json.len());
    println!();

    // Create ChunkReader that reads in small chunks (simulates network streaming)
    let reader = ChunkReader::new(json, 8); // Read 8 bytes at a time

    // Create StreamParser with a reasonably sized buffer
    let mut buffer = [0u8; 256];
    let buffer_size = buffer.len();
    let mut parser = StreamParser::new(reader, &mut buffer);

    println!("🔄 Starting StreamParser with streaming ChunkReader:");
    println!("   Buffer size: {} bytes", buffer_size);
    println!("   Chunk size: 8 bytes (simulates small network packets)");
    println!();

    let mut event_count = 0;
    loop {
        match parser.next_event() {
            Ok(event) => {
                event_count += 1;
                match event {
                    Event::StartObject => println!("  🏁 StartObject"),
                    Event::EndObject => println!("  🏁 EndObject"),
                    Event::StartArray => println!("  📋 StartArray"),
                    Event::EndArray => println!("  📋 EndArray"),
                    Event::Key(key) => {
                        println!("  🔑 Key: '{}'", key.as_str());
                    }
                    Event::String(s) => {
                        println!("  📝 String: '{}'", s.as_str());
                    }
                    Event::Number(num) => {
                        println!("  🔢 Number: {}", num);
                    }
                    Event::Bool(b) => {
                        println!("  ✅ Bool: {}", b);
                    }
                    Event::Null => {
                        println!("  ⭕ Null");
                    }
                    Event::EndDocument => {
                        println!("  🏁 EndDocument");
                        break;
                    }
                }
            }
            Err(e) => {
                println!("❌ Parse error: {:?}", e);
                return Err(e);
            }
        }
    }

    println!();
    println!(
        "✅ Successfully parsed {} events with StreamParser!",
        event_count
    );
    println!("💡 Notice how the Reader was called multiple times in small chunks,");
    println!("   demonstrating true streaming behavior with a fixed-size buffer.");
    Ok(())
}
