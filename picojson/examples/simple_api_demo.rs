// Example demonstrating the simple new API

use picojson::{Event, ParseError, PullParser, SliceParser};

fn main() -> Result<(), ParseError> {
    // Test the new simple API
    let json = r#"{"name": "value", "number": 42, "flag": true}"#;
    let mut parser = SliceParser::new(json);
    println!("Using SliceParser::new() - simple API:");
    println!("Input: {}", json);

    while let Some(event) = parser.next() {
        match event? {
            Event::StartObject => println!("StartObject"),
            Event::EndObject => println!("EndObject"),
            Event::Key(key) => {
                println!("Key: '{}'", key);
            }
            Event::String(s) => {
                println!("String: '{}'", s);
            }
            Event::Number(num) => {
                // Now with ergonomic Display trait - shows parsed value when available, raw string otherwise
                println!("Number: {}", num);
            }
            Event::Bool(b) => {
                println!("Bool: {}", b);
            }
            Event::EndDocument => {
                println!("EndDocument");
                break;
            }
            other => println!("Other: {:?}", other),
        }
    }

    println!();
    println!("✅ Successfully parsed with simple API!");
    Ok(())
}
