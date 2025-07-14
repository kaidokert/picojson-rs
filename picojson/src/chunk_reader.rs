//! A convenience Reader implementation for common use cases.
//!
//! This module provides [`ChunkReader`], a flexible [`Reader`] for parsing
//! data from in-memory byte slices. It can be used to parse a complete
//! JSON document at once or to simulate streaming by reading the data in
//! fixed-size chunks.
//!
//! For production use cases involving I/O, you'll typically implement the
//! [`Reader`] trait for your specific input source:
//!
//! - **File I/O**: `impl Reader for std::fs::File` or `std::io::BufReader`
//! - **Network**: `impl Reader for TcpStream` or async stream adapters
//! - **UART/Serial**: `impl Reader for SerialPort` in embedded systems
//! - **Custom buffers**: Ring buffers, memory-mapped files, etc.
//!
//! # Examples
//!
//! ```rust
//! use picojson::{StreamParser, ChunkReader, PullParser};
//!
//! // Reading from a byte literal, consuming the whole slice at once
//! let json = br#"{"name": "Alice", "age": 30}"#;
//! let mut buffer = [0u8; 256];
//! let reader = ChunkReader::full_slice(json); // Use full_slice for non-streamed data
//! let mut parser = StreamParser::new(reader, &mut buffer);
//!
//! // Process events...
//! while let Some(event) = parser.next() {
//!     match event {
//!         Ok(event) => println!("Event: {:?}", event),
//!         Err(e) => eprintln!("Parse error: {:?}", e),
//!     }
//! }
//! ```

use crate::Reader;

/// A [`Reader`] that reads from a byte slice, optionally in fixed-size chunks.
///
/// This reader can be used in two primary ways:
///
/// 1.  **Full Slice Reading**: By using [`ChunkReader::full_slice()`], it will read
///     from the entire byte slice as fast as the parser's buffer allows. This is
///     ideal for testing, small documents, or when JSON data is already fully
///     loaded in memory.
///
/// 2.  **Chunked Reading**: By using [`ChunkReader::new()`], it limits each `read()`
///     call to a maximum chunk size. This is useful for simulating real-world
///     streaming scenarios (like network packets or file reads) and for stress-testing
///     the parser's buffer management.
///
/// # Example: Full Slice
///
/// ```rust
/// use picojson::{StreamParser, ChunkReader};
///
/// let json_data = br#"{"status": "ok"}"#;
/// let reader = ChunkReader::full_slice(json_data); // Behaves like a simple slice reader
/// let mut buffer = [0u8; 64];
/// let mut parser = StreamParser::new(reader, &mut buffer);
///
/// // Parse normally...
/// ```
///
/// # Example: Chunked Reading
///
/// ```rust
/// use picojson::{StreamParser, ChunkReader};
///
/// let json = br#"{"large": "document with lots of data..."}"#;
/// // Simulate reading only 4 bytes at a time
/// let reader = ChunkReader::new(json, 4);
/// let mut buffer = [0u8; 128];
/// let mut parser = StreamParser::new(reader, &mut buffer);
///
/// // Parser will receive data in small chunks, testing streaming logic
/// ```
#[derive(Debug)]
pub struct ChunkReader<'a> {
    data: &'a [u8],
    pos: usize,
    chunk_size: usize,
}

impl<'a> ChunkReader<'a> {
    /// Create a new chunked reader from a byte slice.
    ///
    /// Each call to `read()` will return at most `chunk_size` bytes,
    /// even if more data is available and the buffer can hold more.
    ///
    /// # Arguments
    ///
    /// * `data` - The byte slice containing JSON data
    /// * `chunk_size` - Maximum bytes to return per read() call (minimum 1)
    pub fn new(data: &'a [u8], chunk_size: usize) -> Self {
        Self {
            data,
            pos: 0,
            chunk_size: chunk_size.max(1), // Ensure at least 1 byte per read
        }
    }

    /// Create a new reader that consumes the entire byte slice at once.
    ///
    /// This is a convenience constructor that configures the reader to consume the
    /// entire slice in a single `read()` operation, behaving like a traditional
    /// slice reader. This is useful for testing and parsing complete in-memory
    /// JSON documents.
    ///
    /// # Arguments
    ///
    /// * `data` - The byte slice containing JSON data
    pub fn full_slice(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            chunk_size: usize::MAX,
        }
    }
}

impl<'a> Reader for ChunkReader<'a> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining_len = self.data.len().saturating_sub(self.pos);
        if remaining_len == 0 {
            return Ok(0);
        }

        let to_copy = remaining_len.min(buf.len()).min(self.chunk_size);

        if to_copy > 0 {
            // Manual byte-by-byte copy to avoid panics
            for i in 0..to_copy {
                if let (Some(dest_byte), Some(src_byte)) =
                    (buf.get_mut(i), self.data.get(self.pos + i))
                {
                    *dest_byte = *src_byte;
                } else {
                    // Should be logically impossible due to `to_copy` calculation, but acts as a safeguard
                    return Ok(i);
                }
            }
            self.pos = self.pos.saturating_add(to_copy);
        }

        Ok(to_copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_slice_reader_basic() {
        let data = b"hello world";
        let mut reader = ChunkReader::full_slice(data);

        // Read some data
        let mut buf = [0u8; 5];
        assert_eq!(reader.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf, b"hello");

        // Read remaining data
        let mut buf = [0u8; 10];
        assert_eq!(reader.read(&mut buf).unwrap(), 6);
        assert_eq!(&buf[..6], b" world");

        // EOF
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn test_full_slice_reader_empty() {
        let mut reader = ChunkReader::full_slice(b"");
        let mut buf = [0u8; 10];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn test_chunk_reader_basic() {
        let data = b"hello world";
        let mut reader = ChunkReader::new(data, 3);

        // First chunk: limited by chunk_size
        let mut buf = [0u8; 10];
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], b"hel");

        // Second chunk: limited by chunk_size
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], b"lo ");

        // Third chunk: limited by chunk_size
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], b"wor");

        // Fourth chunk: limited by remaining data (2 < chunk_size)
        assert_eq!(reader.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], b"ld");

        // EOF
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn test_chunk_reader_small_buffer() {
        let data = b"hello";
        let mut reader = ChunkReader::new(data, 10); // chunk_size > data

        // Limited by buffer size, not chunk_size
        let mut buf = [0u8; 3];
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf, b"hel");

        // Remaining data
        assert_eq!(reader.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], b"lo");
    }

    #[test]
    fn test_chunk_reader_zero_chunk_size() {
        // Should be clamped to 1
        let mut reader = ChunkReader::new(b"hello", 0);

        let mut buf = [0u8; 10];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'h');
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::{Event, PullParser, StreamParser};

    #[test]
    fn test_full_slice_reader_with_stream_parser() {
        let json = br#"{"name": "Alice", "age": 30, "active": true}"#;
        let reader = ChunkReader::full_slice(json);
        let mut buffer = [0u8; 128];
        let mut parser = StreamParser::new(reader, &mut buffer);

        // Parse and verify events
        assert_eq!(parser.next_event().unwrap(), Event::StartObject);
        assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
        assert!(matches!(parser.next_event().unwrap(), Event::String(_)));
        assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
        assert!(matches!(parser.next_event().unwrap(), Event::Number(_)));
        assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
        assert_eq!(parser.next_event().unwrap(), Event::Bool(true));
        assert_eq!(parser.next_event().unwrap(), Event::EndObject);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_chunk_reader_with_stream_parser() {
        let json = br#"[1, 2, 3, 4, 5]"#;
        let reader = ChunkReader::new(json, 2); // VERY small chunks - this triggered the bug
        let mut buffer = [0u8; 64]; // Small buffer - this triggered the bug
        let mut parser = StreamParser::new(reader, &mut buffer);

        // Should parse correctly despite tiny chunks
        assert_eq!(parser.next_event().unwrap(), Event::StartArray);
        for i in 1..=5 {
            let event = parser.next_event().unwrap();
            if let Event::Number(num) = event {
                let parsed: i32 = num.as_str().parse().unwrap();
                assert_eq!(parsed, i);
            } else {
                panic!("Expected number, got: {:?}", event);
            }
        }
        assert_eq!(parser.next_event().unwrap(), Event::EndArray);
        assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
    }

    #[test]
    fn test_stress_test_tiny_chunks() {
        // Test with 1-byte chunks to stress the streaming logic
        let json = br#"{"escape": "hello\nworld", "unicode": "\u0041"}"#;
        let reader = ChunkReader::new(json, 1);
        let mut buffer = [0u8; 128];
        let mut parser = StreamParser::new(reader, &mut buffer);

        let mut event_count = 0;
        while let Ok(event) = parser.next_event() {
            event_count += 1;
            if matches!(event, Event::EndDocument) {
                break;
            }
        }

        assert!(event_count > 5); // Should have parsed multiple events
    }

    // Helper function for testing delimiter bug boundary conditions
    fn test_delimiter_bug_helper(chunk_size: usize, buffer_size: usize) {
        let json = br#"[1, 2, 3]"#;
        let reader = ChunkReader::new(json, chunk_size);
        let mut buffer = vec![0u8; buffer_size];
        let mut parser = StreamParser::new(reader, &mut buffer);

        assert_eq!(parser.next_event().unwrap(), Event::StartArray);

        // Process ALL events to see the complete pattern
        let mut number_count = 0;
        loop {
            let event = parser.next_event().unwrap();
            match event {
                Event::Number(num) => {
                    number_count += 1;
                    let num_str = num.as_str();

                    // Check for delimiter contamination
                    if num_str.contains(',') || num_str.contains(']') {
                        panic!("Number {} contains delimiter: '{}'", number_count, num_str);
                    }

                    let parsed: i32 = num_str
                        .parse()
                        .expect(&format!("Failed to parse '{}' as number", num_str));
                    assert_eq!(
                        parsed, number_count,
                        "Number {} should be {}, got {}",
                        number_count, number_count, parsed
                    );
                }
                Event::EndArray => {
                    break;
                }
                Event::EndDocument => {
                    break;
                }
                _ => {}
            }
        }

        // Finish with EndDocument if we stopped at EndArray
        if let Ok(Event::EndDocument) = parser.next_event() {}

        assert_eq!(number_count, 3, "Should have processed exactly 3 numbers");
    }

    #[test]
    fn test_delimiter_bug_tiny_chunks_small_buffer() {
        // Original failing condition: 2-byte chunks + 64-byte buffer
        test_delimiter_bug_helper(2, 64);
    }

    #[test]
    fn test_delimiter_bug_tiny_chunks_medium_buffer() {
        // Test if larger buffer fixes it: 2-byte chunks + 128-byte buffer
        test_delimiter_bug_helper(2, 128);
    }

    #[test_log::test]
    fn test_delimiter_bug_small_chunks_small_buffer() {
        // Test if larger chunks fix it: 3-byte chunks + 64-byte buffer
        test_delimiter_bug_helper(3, 64);
    }

    #[test]
    fn test_delimiter_bug_medium_chunks_small_buffer() {
        // Test boundary: 4-byte chunks + 64-byte buffer
        test_delimiter_bug_helper(4, 64);
    }

    #[test]
    fn test_delimiter_bug_large_chunks_small_buffer() {
        // Test normal working condition: 8-byte chunks + 64-byte buffer
        test_delimiter_bug_helper(8, 64);
    }

    #[test]
    fn test_delimiter_bug_tiny_chunks_large_buffer() {
        // Test demo condition: 2-byte chunks + 256-byte buffer
        test_delimiter_bug_helper(2, 256);
    }
}
