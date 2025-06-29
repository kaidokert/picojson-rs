// Example demonstrating the simple new API

use stax::{Event, PullParser};

fn main() -> Result<(), stax::ParseError> {
    // Test the new simple API
    let json = r#"{"name": "value", "number": 42, "flag": true}"#;
    let mut parser = PullParser::new(json);
    println!("Using PullParser::new() - simple API:");
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
