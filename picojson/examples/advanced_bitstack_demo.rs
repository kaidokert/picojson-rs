// Example demonstrating configurable BitStack storage using BitStackConfig API

use picojson::{BitStackStruct, Event, ParseError, PullParser, SliceParser};

fn main() -> Result<(), ParseError> {
    println!("BitStack Configuration Examples");
    println!("===============================");

    // Test 1: Default SliceParser (uses DefaultConfig: u32 BitStack and u8 depth counter)
    println!("1. Standard SliceParser (u32 BitStack, ~32 levels max):");
    let json = r#"{"deeply": {"nested": {"object": {"with": {"data": "test"}}}}}"#;
    let mut scratch = [0u8; 512];
    let mut parser = SliceParser::with_buffer(json, &mut scratch);
    let mut depth = 0;
    while let Some(event) = parser.next() {
        match event? {
            Event::StartObject => {
                depth += 1;
                println!("  {}StartObject (depth: {})", "  ".repeat(depth), depth);
            }
            Event::EndObject => {
                println!("  {}EndObject (depth: {})", "  ".repeat(depth), depth);
                depth -= 1;
            }
            Event::Key(k) => println!("  {}Key: {:?}", "  ".repeat(depth + 1), &*k),
            Event::String(s) => println!("  {}String: {:?}", "  ".repeat(depth + 1), &*s),
            Event::EndDocument => break,
            _ => {}
        }
    }
    println!("  Maximum depth reached: {}\n", depth);

    // Test 2: u8 BitStack (8-bit depth, more memory efficient for shallow data)
    println!("2. Memory-efficient SliceParser (u8 BitStack, ~8 levels max):");
    let json = r#"{"shallow": {"data": [1, 2, 3]}}"#;
    let mut scratch = [0u8; 256];
    let mut parser =
        SliceParser::<BitStackStruct<u8, u8>>::with_config_and_buffer(json, &mut scratch);
    let mut depth = 0;
    while let Some(event) = parser.next() {
        match event? {
            Event::StartObject => {
                depth += 1;
                println!("  {}StartObject (depth: {})", "  ".repeat(depth), depth);
            }
            Event::StartArray => {
                depth += 1;
                println!("  {}StartArray (depth: {})", "  ".repeat(depth), depth);
            }
            Event::EndObject => {
                println!("  {}EndObject (depth: {})", "  ".repeat(depth), depth);
                depth -= 1;
            }
            Event::EndArray => {
                println!("  {}EndArray (depth: {})", "  ".repeat(depth), depth);
                depth -= 1;
            }
            Event::Key(k) => println!("  {}Key: {:?}", "  ".repeat(depth + 1), &*k),
            Event::Number(n) => println!("  {}Number: {}", "  ".repeat(depth + 1), n),
            Event::EndDocument => break,
            _ => {}
        }
    }
    println!("  Maximum depth reached: {}\n", depth);

    // Test 3: u64 BitStack (64-bit depth, for very deep nesting)
    println!("3. Deep-nesting SliceParser (u64 BitStack, ~64 levels max):");
    let json = r#"{"very": {"deeply": {"nested": {"structure": {"with": {"many": {"levels": {"data": "deep"}}}}}}}}"#;
    let mut scratch = [0u8; 1024];
    let mut parser =
        SliceParser::<BitStackStruct<u64, u16>>::with_config_and_buffer(json, &mut scratch);
    let mut depth = 0;
    while let Some(event) = parser.next() {
        match event? {
            Event::StartObject => {
                depth += 1;
                println!("  {}StartObject (depth: {})", "  ".repeat(depth), depth);
            }
            Event::EndObject => {
                println!("  {}EndObject (depth: {})", "  ".repeat(depth), depth);
                depth -= 1;
            }
            Event::Key(k) => println!("  {}Key: {:?}", "  ".repeat(depth + 1), &*k),
            Event::String(s) => println!("  {}String: {:?}", "  ".repeat(depth + 1), &*s),
            Event::EndDocument => break,
            _ => {}
        }
    }
    println!("  Maximum depth reached: {}", depth);

    Ok(())
}
