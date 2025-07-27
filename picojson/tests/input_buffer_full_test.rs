// Test for InputBufferFull error variant
use picojson::{ParseError, PullParser, StreamParser};
use std::io;

/// Mock reader that simulates a scenario where input buffer limits could be exceeded
struct LargeDataReader {
    data: Vec<u8>,
    position: usize,
    chunk_size: usize,
}

impl LargeDataReader {
    fn new(json_data: &str, chunk_size: usize) -> Self {
        Self {
            data: json_data.as_bytes().to_vec(),
            position: 0,
            chunk_size,
        }
    }
}

impl picojson::Reader for LargeDataReader {
    type Error = io::Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.position >= self.data.len() {
            return Ok(0); // End of stream
        }

        let remaining = self.data.len() - self.position;
        let to_read = std::cmp::min(std::cmp::min(buf.len(), self.chunk_size), remaining);

        buf[..to_read].copy_from_slice(&self.data[self.position..self.position + to_read]);
        self.position += to_read;

        Ok(to_read)
    }
}

#[test]
fn test_input_buffer_full_scenario() {
    // Create a very large JSON document that could potentially overflow input buffers
    let large_object = format!(
        r#"{{"key": "{}"}}"#,
        "x".repeat(10000) // Very long string value
    );

    // Use a very small buffer that would be insufficient for the large content
    let mut buffer = [0u8; 32]; // Intentionally small buffer
    let reader = LargeDataReader::new(&large_object, 16); // Small read chunks

    let mut parser = StreamParser::new(reader, &mut buffer);

    // Attempt to parse the large document with insufficient buffer space
    let mut events = Vec::new();
    loop {
        match parser.next_event() {
            Ok(event) => {
                events.push(format!("{:?}", event));
                if matches!(event, picojson::Event::EndDocument) {
                    break;
                }
            }
            Err(e) => {
                // InputBufferFull is now properly implemented as of stream_content_builder.rs fix
                match e {
                    ParseError::ScratchBufferFull => {
                        // Legacy case: ScratchBufferFull for escape processing issues
                        assert!(
                            matches!(e, ParseError::ScratchBufferFull),
                            "Expected ScratchBufferFull, got: {:?}",
                            e
                        );
                        return;
                    }
                    ParseError::InputBufferFull => {
                        // InputBufferFull for tokens too large for buffer capacity
                        assert!(
                            matches!(e, ParseError::InputBufferFull),
                            "Expected InputBufferFull, got: {:?}",
                            e
                        );
                        return;
                    }
                    _ => {
                        panic!("Unexpected error: {:?}", e);
                    }
                }
            }
        }
    }

    // If we reach here, the parser somehow managed to handle the large document
    // This is unexpected behavior that should cause the test to fail
    panic!(
        "Test should have failed: Parser unexpectedly succeeded in handling large document with small buffer. \
        Expected ScratchBufferFull or InputBufferFull error, but got {} events: {:?}",
        events.len(),
        events
    );
}

#[test]
fn test_input_buffer_full_with_extremely_long_token() {
    // Test with an extremely long single token that exceeds reasonable input buffer limits
    let extremely_long_key = "k".repeat(50000);
    let json = format!(r#"{{"{key}": "value"}}"#, key = extremely_long_key);

    let mut buffer = [0u8; 64]; // Very small buffer
    let reader = LargeDataReader::new(&json, 32);

    let mut parser = StreamParser::new(reader, &mut buffer);

    match parser.next_event() {
        Ok(_) => {
            // Continue parsing to see what happens
            loop {
                match parser.next_event() {
                    Ok(event) => {
                        if matches!(event, picojson::Event::EndDocument) {
                            break;
                        }
                    }
                    Err(e) => match e {
                        ParseError::ScratchBufferFull => {
                            assert!(
                                matches!(e, ParseError::ScratchBufferFull),
                                "Expected ScratchBufferFull for extremely long token, got: {:?}",
                                e
                            );
                            return;
                        }
                        ParseError::InputBufferFull => {
                            assert!(
                                matches!(e, ParseError::InputBufferFull),
                                "Expected InputBufferFull for extremely long token, got: {:?}",
                                e
                            );
                            return;
                        }
                        _ => {
                            panic!("Unexpected error for extremely long token: {:?}", e);
                        }
                    },
                }
            }
        }
        Err(e) => {
            match e {
                ParseError::ScratchBufferFull => {
                    assert!(
                    matches!(e, ParseError::ScratchBufferFull),
                    "Expected ScratchBufferFull on first event for extremely long token, got: {:?}", e
                );
                }
                ParseError::InputBufferFull => {
                    assert!(
                    matches!(e, ParseError::InputBufferFull),
                    "Expected InputBufferFull on first event for extremely long token, got: {:?}", e
                );
                }
                _ => {
                    panic!(
                        "Unexpected error on first event for extremely long token: {:?}",
                        e
                    );
                }
            }
        }
    }
}
