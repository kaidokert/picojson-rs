// Debug root-level number parsing issue
use picojson::{ChunkReader, Event, PullParser, SliceParser, StreamParser};

fn test_json(input: &str, description: &str) {
    println!("\n=== Testing: {} ===", description);
    println!("Input: '{}'", input);

    let mut scratch = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(input, &mut scratch);

    let mut event_count = 0;
    loop {
        match parser.next_event() {
            Ok(event) => {
                event_count += 1;
                println!("Event {}: {:?}", event_count, event);
                if matches!(event, Event::EndDocument) {
                    break;
                }
                if event_count > 10 {
                    println!("Too many events, stopping...");
                    break;
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                break;
            }
        }
    }
    println!("Total events: {}", event_count);
}

#[test]
fn debug_root_level_numbers() {
    // Test root-level primitives
    test_json("42", "Root number");
    test_json(r#""hello""#, "Root string");
    test_json("true", "Root boolean true");
    test_json("false", "Root boolean false");
    test_json("null", "Root null");

    // Compare with structured JSON
    test_json(r#"{"value": 42}"#, "Small number in object");
    test_json(r#"{"value": 9999999999}"#, "Large number in object");
    test_json("[42]", "Small number in array");
    test_json("[9999999999]", "Large number in array");
}

#[test]
fn debug_lonely_int() {
    println!("=== DEBUGGING LONELY INT: '42' ===");

    // Test with SliceParser with debug logging
    println!("\n--- SliceParser with RUST_LOG=debug ---");
    let input = "42";
    let mut scratch = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(input, &mut scratch);

    match parser.next_event() {
        Ok(event) => {
            println!("SliceParser event: {:?}", event);
            if let Event::Number(num) = &event {
                println!("Number str: '{}'", num.as_str());
                println!("Number as_int: {:?}", num.as_int());
            }
        }
        Err(e) => {
            println!("SliceParser error: {:?}", e);
        }
    }

    // Test with StreamParser
    println!("\n--- StreamParser ---");
    let input = "42";
    let mut scratch = [0u8; 1024];
    let reader = ChunkReader::new(input.as_bytes(), 1024);
    let mut parser = StreamParser::new(reader, &mut scratch);

    match parser.next_event() {
        Ok(event) => {
            println!("StreamParser event: {:?}", event);
            if let Event::Number(num) = &event {
                println!("Number str: '{}'", num.as_str());
                println!("Number as_int: {:?}", num.as_int());
            }
        }
        Err(e) => {
            println!("StreamParser error: {:?}", e);
        }
    }
}

#[test]
fn debug_lonely_negative_real() {
    println!("=== DEBUGGING LONELY NEGATIVE REAL: '-0.1' ===");

    // Test with SliceParser
    println!("\n--- SliceParser ---");
    let input = "-0.1";
    let mut scratch = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(input, &mut scratch);

    match parser.next_event() {
        Ok(event) => {
            println!("SliceParser event: {:?}", event);
            if let Event::Number(num) = &event {
                println!("Number str: '{}'", num.as_str());
                println!("Number as_int: {:?}", num.as_int());
                #[cfg(feature = "float")]
                println!("Number as_f64: {:?}", num.as_f64());
            }
        }
        Err(e) => {
            println!("SliceParser error: {:?}", e);
        }
    }

    // Test with StreamParser
    println!("\n--- StreamParser ---");
    let input = "-0.1";
    let mut scratch = [0u8; 1024];
    let reader = ChunkReader::new(input.as_bytes(), 1024);
    let mut parser = StreamParser::new(reader, &mut scratch);

    match parser.next_event() {
        Ok(event) => {
            println!("StreamParser event: {:?}", event);
            if let Event::Number(num) = &event {
                println!("Number str: '{}'", num.as_str());
                println!("Number as_int: {:?}", num.as_int());
                #[cfg(feature = "float")]
                println!("Number as_f64: {:?}", num.as_f64());
            }
        }
        Err(e) => {
            println!("StreamParser error: {:?}", e);
        }
    }
}
