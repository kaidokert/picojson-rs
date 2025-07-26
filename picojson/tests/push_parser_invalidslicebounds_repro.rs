// SPDX-License-Identifier: Apache-2.0

//! Minimal reproduction test for InvalidSliceBounds buffer boundary tracking issue
//! This test aims to reproduce the exact same error that occurs in pass1.json parsing

use picojson::{DefaultConfig, Event, PushParser, PushParserHandler};

/// Simple handler that collects events for verification
struct ReproHandler {
    events: Vec<String>,
}

impl ReproHandler {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, String> for ReproHandler {
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), String> {
        // Convert to owned event for storage
        let event_str = match event {
            Event::StartObject => "StartObject".to_string(),
            Event::EndObject => "EndObject".to_string(),
            Event::StartArray => "StartArray".to_string(),
            Event::EndArray => "EndArray".to_string(),
            Event::Key(k) => format!("Key({})", k.as_ref()),
            Event::String(s) => format!("String({})", s.as_ref()),
            Event::Number(n) => format!("Number({})", n.as_str()),
            Event::Bool(b) => format!("Bool({})", b),
            Event::Null => "Null".to_string(),
            Event::EndDocument => "EndDocument".to_string(),
        };

        self.events.push(event_str);
        Ok(())
    }
}

#[test]
fn test_reproduce_invalidslicebounds_minimal() {
    // Start with the exact content from pass1.json that might cause issues
    let json_content = br#"{"hex": "\u0123\u4567\u89AB\uCDEF\uabcd\uef4A"}"#;

    // Use a small buffer that might trigger the boundary issue
    let mut buffer = [0u8; 32];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    let result = parser.write(json_content);
    match result {
        Ok(()) => {
            let finish_result = parser.finish();
            match finish_result {
                Ok(()) => {
                    let handler = parser.destroy();
                    println!("SUCCESS: Events = {:?}", handler.events);
                }
                Err(e) => {
                    panic!("FINISH ERROR: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("WRITE ERROR: {:?}", e);
        }
    }
}

#[test]
fn test_reproduce_invalidslicebounds_chunked() {
    // Try the same content but in smaller chunks to trigger buffer boundary issues
    let json_content = br#"{"hex": "\u0123\u4567\u89AB\uCDEF\uabcd\uef4A"}"#;

    // Use a buffer large enough for the content but small enough to test chunking
    let mut buffer = [0u8; 32];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    // Write in small chunks
    let chunk_size = 8;
    for chunk in json_content.chunks(chunk_size) {
        let result = parser.write(chunk);
        match result {
            Ok(()) => continue,
            Err(e) => {
                panic!("CHUNK WRITE ERROR: {:?}", e);
            }
        }
    }

    let finish_result = parser.finish();
    match finish_result {
        Ok(()) => {
            let handler = parser.destroy();
            println!("SUCCESS: Events = {:?}", handler.events);
        }
        Err(e) => {
            panic!("FINISH ERROR: {:?}", e);
        }
    }
}

#[test]
fn test_reproduce_invalidslicebounds_complex_key() {
    // Try the complex key from pass1.json line 45 that might cause issues
    let json_content = br#"{"\/\\\"\uCAFE\uBABE\uAB98\uFCDE\ubcda\uef4A\b\f\n\r\t": "value"}"#;

    // Use a small buffer
    let mut buffer = [0u8; 32];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    let result = parser.write(json_content);
    match result {
        Ok(()) => {
            let finish_result = parser.finish();
            match finish_result {
                Ok(()) => {
                    let handler = parser.destroy();
                    println!("SUCCESS: Events = {:?}", handler.events);
                }
                Err(e) => {
                    panic!("FINISH ERROR: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("WRITE ERROR: {:?}", e);
        }
    }
}

#[test]
fn test_reproduce_invalidslicebounds_exact_pass1() {
    // Use the exact pass1.json content
    let json_content = include_bytes!("data/json_checker/pass1.json");

    // Use a small buffer to stress the boundary handling
    let mut buffer = [0u8; 64];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    let result = parser.write(json_content);
    match result {
        Ok(()) => {
            let finish_result = parser.finish();
            match finish_result {
                Ok(()) => {
                    let handler = parser.destroy();
                    println!("SUCCESS: Parsed {} events", handler.events.len());
                }
                Err(e) => {
                    panic!("FINISH ERROR: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("WRITE ERROR: {:?}", e);
        }
    }
}

#[test]
fn test_debug_unicode_processing() {
    use picojson::{Event, PullParser, SliceParser};

    // Debug the exact unicode processing for \u0041 -> A
    let json = r#"{"k":"\u0041"}"#;
    println!("Testing unicode: {} (should produce 'A')", json);

    // First test with SliceParser (known working)
    println!("\n=== SliceParser (working reference) ===");
    let mut buffer = [0u8; 64];
    let mut parser = SliceParser::with_buffer(json, &mut buffer);

    loop {
        match parser.next_event() {
            Ok(Event::EndDocument) => break,
            Ok(event) => println!("SliceParser event: {:?}", event),
            Err(e) => {
                println!("SliceParser error: {:?}", e);
                break;
            }
        }
    }

    // Now test with PushParser (broken)
    println!("\n=== PushParser (broken) ===");
    let mut buffer = [0u8; 64];
    let handler = ReproHandler::new();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    let result = parser.write(json.as_bytes());
    match result {
        Ok(()) => {
            let finish_result = parser.finish();
            match finish_result {
                Ok(()) => {
                    let handler = parser.destroy();
                    println!("PushParser events: {:?}", handler.events);

                    // Check if we got the expected "A" character
                    let expected_char = "A";
                    let found_char = handler
                        .events
                        .iter()
                        .find(|event| event.starts_with("String("))
                        .map(|event| &event[7..event.len() - 1]); // Extract content between String( and )

                    if let Some(actual) = found_char {
                        if actual == expected_char {
                            println!(
                                "✅ Unicode correctly processed: {} -> {}",
                                "\\u0041", actual
                            );
                        } else {
                            println!(
                                "❌ Unicode incorrectly processed: {} -> {} (expected {})",
                                "\\u0041", actual, expected_char
                            );
                        }
                    }
                }
                Err(e) => {
                    println!("❌ FINISH ERROR: {:?}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ WRITE ERROR: {:?}", e);
        }
    }
}

#[test]
fn test_reproduce_invalidslicebounds_minimal_case() {
    // Try the most minimal case that still has unescaped content
    let test_cases = vec![
        (r#"{"k":""}"#, "empty string"),
        (r#"{"k":"a"}"#, "single char"),
        (r#"{"k":"ab"}"#, "two chars"),
        (r#"{"k":"\n"}"#, "simple escape"),
        (r#"{"k":"\u0041"}"#, "single unicode"),
        (r#"{"k":"\u0123"}"#, "single unicode hex"),
        (r#"{"k":"\u0123\u4567"}"#, "double unicode"),
    ];

    for (json, description) in test_cases {
        println!("Testing: {} - {}", description, json);
        let json_bytes = json.as_bytes();

        // Use a small buffer to trigger the issue
        let mut buffer = [0u8; 32];
        let handler = ReproHandler::new();
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        let result = parser.write(json_bytes);
        match result {
            Ok(()) => {
                let finish_result = parser.finish();
                match finish_result {
                    Ok(()) => {
                        let handler = parser.destroy();
                        println!(
                            "✅ SUCCESS: {} - Events = {:?}",
                            description, handler.events
                        );
                    }
                    Err(e) => {
                        println!("❌ FINISH ERROR: {} - {:?}", description, e);
                    }
                }
            }
            Err(e) => {
                println!("❌ WRITE ERROR: {} - {:?}", description, e);
                return; // Stop at first error to identify the minimal trigger
            }
        }
        println!();
    }
}

#[test]
fn test_reproduce_invalidslicebounds_progressive_size() {
    // Test with progressively smaller buffer sizes to find the exact trigger point
    let json_content = br#"{"key": "\u0123\u4567"}"#;

    for buffer_size in (8..=64).rev() {
        println!("Trying buffer size: {}", buffer_size);
        let mut buffer = vec![0u8; buffer_size];
        let handler = ReproHandler::new();
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        let result = parser.write(json_content);
        match result {
            Ok(()) => {
                let finish_result = parser.finish();
                match finish_result {
                    Ok(()) => {
                        let handler = parser.destroy();
                        println!(
                            "SUCCESS at buffer size {}: Events = {:?}",
                            buffer_size, handler.events
                        );
                    }
                    Err(e) => {
                        println!("FINISH ERROR at buffer size {}: {:?}", buffer_size, e);
                        break;
                    }
                }
            }
            Err(e) => {
                println!("WRITE ERROR at buffer size {}: {:?}", buffer_size, e);
                break;
            }
        }
    }
}
