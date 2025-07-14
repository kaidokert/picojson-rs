#[cfg(test)]
mod debug_position_test {
    use crate::{ChunkReader, Event, PullParser, StreamParser};

    #[test_log::test]
    fn debug_simple_escape_position() {
        // Very simple case: just track positions
        let json = b"\"a\\nb\"";
        println!("JSON bytes: {:?}", json);
        for (i, &b) in json.iter().enumerate() {
            println!("  pos {}: '{}' ({})", i, b as char, b);
        }

        let reader = ChunkReader::new(json, 8);
        let mut buffer = [0u8; 32];
        let mut parser = StreamParser::<_, crate::ujson::DefaultConfig>::new(reader, &mut buffer);

        if let Event::String(s) = parser.next_event().unwrap() {
            println!("Result: {:?}", s.as_str());
            assert_eq!(s.as_str(), "a\nb");
        } else {
            panic!("Expected String event");
        }
    }
}
