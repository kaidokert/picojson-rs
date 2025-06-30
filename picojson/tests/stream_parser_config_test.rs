// Integration test for StreamParser configurability
use picojson::{ArrayBitStack, BitStackStruct, Event, Reader, StreamParser};

/// Test Reader that reads from a byte slice in small chunks
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
            chunk_size,
        }
    }
}

impl Reader for ChunkReader<'_> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let remaining = &self.data[self.pos..];
        let to_copy = buf.len().min(remaining.len()).min(self.chunk_size);
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

#[test]
fn test_stream_parser_default_config() {
    let json = b"{\"name\": \"test\"}";
    let reader = ChunkReader::new(json, 4);
    let mut buffer = [0u8; 128];

    // Default configuration: uses DefaultConfig (u32 bucket, u8 counter)
    let mut parser = StreamParser::new(reader, &mut buffer);

    assert_eq!(parser.next_event().unwrap(), Event::StartObject);
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
    assert!(matches!(parser.next_event().unwrap(), Event::String(_)));
    assert_eq!(parser.next_event().unwrap(), Event::EndObject);
    assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
}

#[test]
fn test_stream_parser_custom_bitstack_config() {
    let json = b"{\"value\": 42}";
    let reader = ChunkReader::new(json, 3);
    let mut buffer = [0u8; 128];

    // Custom configuration: u64 bucket + u16 counter for deeper nesting
    let mut parser = StreamParser::<_, BitStackStruct<u64, u16>>::with_config(reader, &mut buffer);

    assert_eq!(parser.next_event().unwrap(), Event::StartObject);
    assert!(matches!(parser.next_event().unwrap(), Event::Key(_)));
    assert!(matches!(parser.next_event().unwrap(), Event::Number(_)));
    assert_eq!(parser.next_event().unwrap(), Event::EndObject);
    assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
}

#[test]
fn test_stream_parser_array_bitstack_config() {
    let json = b"[true, false]";
    let reader = ChunkReader::new(json, 2);
    let mut buffer = [0u8; 128];

    // ArrayBitStack configuration: 4 elements of u32 + u16 counter for ultra-deep nesting
    let mut parser =
        StreamParser::<_, ArrayBitStack<4, u32, u16>>::with_config(reader, &mut buffer);

    assert_eq!(parser.next_event().unwrap(), Event::StartArray);
    assert_eq!(parser.next_event().unwrap(), Event::Bool(true));
    assert_eq!(parser.next_event().unwrap(), Event::Bool(false));
    assert_eq!(parser.next_event().unwrap(), Event::EndArray);
    assert_eq!(parser.next_event().unwrap(), Event::EndDocument);
}
