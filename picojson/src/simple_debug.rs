#[cfg(test)]
mod simple_debug {
    use crate::chunk_reader::ChunkReader;
    use crate::*;

    #[test_log::test]
    fn trace_hello_escape() {
        // Just trace "hello\n" - first 7 bytes
        let json = b"\"hello\\n\"";
        println!("=== NEW IMPLEMENTATION ===");
        println!("Input bytes: {:?}", json);
        for (i, &b) in json.iter().enumerate() {
            println!("  Position {}: '{}' (byte {})", i, b as char, b);
        }

        let reader = ChunkReader::new(json, 16);
        let mut buffer = [0u8; 64];
        let mut parser =
            stream_parser::StreamParser::<_, crate::ujson::DefaultConfig>::new(reader, &mut buffer);

        match parser.next_event() {
            Ok(Event::String(s)) => {
                println!("SUCCESS: Got string '{}'", s.as_str());
                println!("Expected: 'hello\\n', Got: '{}'", s.as_str());
                if s.as_str() == "hello\n" {
                    println!("✅ Test PASSED");
                } else {
                    println!("❌ Test FAILED");
                }
            }
            Ok(other) => println!("ERROR: Expected String, got {:?}", other),
            Err(e) => println!("ERROR: Parse failed: {:?}", e),
        }
    }
}
