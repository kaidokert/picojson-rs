// Example demonstrating StreamParser with a Reader over a fixed-size array

use picojson::{Event, ParseError, PullParser, Reader, StreamParser};

/// Simple Reader implementation that reads from a fixed-size byte array
/// This simulates reading from a stream, network socket, or any other byte source
struct ArrayReader<'a> {
    data: &'a [u8],
    position: usize,
    chunk_size: usize, // Simulate streaming by reading in chunks
}

impl<'a> ArrayReader<'a> {
    /// Create a new ArrayReader from a byte slice
    /// chunk_size controls how many bytes are read at once (simulates network packets)
    fn new(data: &'a [u8], chunk_size: usize) -> Self {
        Self {
            data,
            position: 0,
            chunk_size,
        }
    }
}

impl<'a> Reader for ArrayReader<'a> {
    type Error = std::io::Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.position);
        if remaining == 0 {
            return Ok(0); // EOF
        }

        // Read at most chunk_size bytes to simulate streaming behavior
        let to_read = remaining.min(buf.len()).min(self.chunk_size);
        let end_pos = self.position + to_read;

        buf[..to_read].copy_from_slice(&self.data[self.position..end_pos]);
        self.position = end_pos;

        println!(
            "  📖 Reader: read {} bytes (pos: {}/{})",
            to_read,
            self.position,
            self.data.len()
        );
        Ok(to_read)
    }
}

fn main() -> Result<(), ParseError> {
    println!("🚀 StreamParser Demo with ArrayReader");
    println!("=====================================");

    // Test JSON with various data types including escape sequences
    let json = br#"{"name": "hello\nworld", "items": [1, 2.5, true, null], "count": 42}"#;

    println!("📄 Input JSON: {}", std::str::from_utf8(json).unwrap());
    println!("📏 Total size: {} bytes", json.len());
    println!();

    // Create ArrayReader that reads in small chunks (simulates network streaming)
    let reader = ArrayReader::new(json, 8); // Read 8 bytes at a time

    // Create StreamParser with a reasonably sized buffer
    let mut buffer = [0u8; 256];
    let buffer_size = buffer.len();
    let mut parser = StreamParser::new(reader, &mut buffer);

    println!("🔄 Starting StreamParser with streaming ArrayReader:");
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
