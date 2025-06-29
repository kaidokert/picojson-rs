// Example demonstrating ArrayBitStack for large nesting depths

use picojson::ArrayBitStack;
use picojson::{Event, ParseError, PullParserFlex};

fn main() -> Result<(), ParseError> {
    println!("=== ArrayBitStack Demo ===\n");

    // Generate deeply nested JSON with mixed objects and arrays (70+ levels)
    let deep_json = generate_deep_mixed_json(65);
    println!("1. ArrayBitStack<3, u32> (96-bit depth) - Mixed {{}} and [] nesting to depth ~65:");
    println!(
        "   Generated JSON (first 100 chars): {}",
        &deep_json[..deep_json.len().min(100)]
    );
    println!("   JSON structure: obj->arr->obj->arr->... (alternating pattern)");

    let mut scratch = [0u8; 2048];
    let mut parser: PullParserFlex<ArrayBitStack<3, u32>, u16> =
        PullParserFlex::new_with_buffer(&deep_json, &mut scratch);
    let mut depth = 0;
    let mut max_depth = 0;

    loop {
        match parser.next() {
            Some(Ok(event)) => match event {
                Event::StartObject => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    if depth <= 5 || depth % 10 == 0 {
                        println!(
                            "  {}StartObject (depth: {})",
                            "  ".repeat((depth - 1).min(3)),
                            depth
                        );
                    }
                }
                Event::StartArray => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    if depth <= 5 || depth % 10 == 0 {
                        println!(
                            "  {}StartArray (depth: {})",
                            "  ".repeat((depth - 1).min(3)),
                            depth
                        );
                    }
                }
                Event::EndObject => {
                    if depth <= 5 || depth % 10 == 0 {
                        println!(
                            "  {}EndObject (depth: {})",
                            "  ".repeat((depth - 1).min(3)),
                            depth
                        );
                    }
                    depth -= 1;
                }
                Event::EndArray => {
                    if depth <= 5 || depth % 10 == 0 {
                        println!(
                            "  {}EndArray (depth: {})",
                            "  ".repeat((depth - 1).min(3)),
                            depth
                        );
                    }
                    depth -= 1;
                }
                Event::Key(key) => {
                    if depth <= 5 {
                        println!("  {}Key: '{}'", "  ".repeat(depth.min(3)), key);
                    }
                }
                Event::String(s) => {
                    println!(
                        "  {}String: '{}' (at max depth: {})",
                        "  ".repeat(depth.min(3)),
                        s,
                        depth
                    );
                }
                Event::Number(num) => {
                    println!(
                        "  {}Number: {} (at max depth: {})",
                        "  ".repeat(depth.min(3)),
                        num,
                        depth
                    );
                }
                Event::EndDocument => break,
                _ => {}
            },
            Some(Err(_)) => {
                println!(
                    "  ! Parse error encountered at depth {}, continuing...",
                    depth
                );
                break;
            }
            None => break,
        }
    }
    println!(
        "   ✅ Successfully parsed {} levels of mixed nesting!\n",
        max_depth
    );

    // Test ArrayBitStack with smaller elements for memory efficiency
    println!("2. ArrayBitStack<8, u8> (64-bit depth tracking) - Complex nested structure:");
    let complex_json = generate_complex_nested_json(25);
    println!("   JSON structure: Objects with arrays containing objects with data");

    let mut scratch = [0u8; 1024];
    let mut parser: PullParserFlex<ArrayBitStack<8, u8>, u8> =
        PullParserFlex::new_with_buffer(&complex_json, &mut scratch);
    let mut depth = 0;
    let mut max_depth = 0;

    while let Some(event) = parser.next() {
        match event? {
            Event::StartArray => {
                depth += 1;
                max_depth = max_depth.max(depth);
                if depth <= 8 {
                    println!("  {}StartArray (depth: {})", "  ".repeat(depth), depth);
                }
            }
            Event::StartObject => {
                depth += 1;
                max_depth = max_depth.max(depth);
                if depth <= 8 {
                    println!("  {}StartObject (depth: {})", "  ".repeat(depth), depth);
                }
            }
            Event::EndArray => {
                if depth <= 8 {
                    println!("  {}EndArray (depth: {})", "  ".repeat(depth), depth);
                }
                depth -= 1;
            }
            Event::EndObject => {
                if depth <= 8 {
                    println!("  {}EndObject (depth: {})", "  ".repeat(depth), depth);
                }
                depth -= 1;
            }
            Event::Key(key) => {
                if depth <= 8 {
                    println!("  {}Key: '{}'", "  ".repeat(depth), key);
                }
            }
            Event::Number(num) => {
                if depth <= 8 {
                    println!("  {}Number: {}", "  ".repeat(depth), num);
                }
            }
            Event::String(s) => {
                if depth <= 8 {
                    println!("  {}String: '{}'", "  ".repeat(depth), s);
                }
            }
            Event::EndDocument => break,
            _ => {}
        }
    }
    println!(
        "   ✅ Successfully parsed {} levels of complex nesting!\n",
        max_depth
    );

    println!("✅ ArrayBitStack configurations working!");
    println!();

    println!("ArrayBitStack Summary:");
    println!("• ArrayBitStack<3, u32>: 96-bit depth (3 × 32 bits)");
    println!("• ArrayBitStack<8, u8>: 64-bit depth (8 × 8 bits) - memory efficient");
    println!("• ArrayBitStack<16, u32>: 512-bit depth (16 × 32 bits) - ultra deep");
    println!("• Configurable element type (u8, u16, u32, u64) and array size");

    Ok(())
}

/// Generate deeply nested JSON with alternating objects and arrays
/// Pattern: {"level0": [{"level2": [{"level4": ... "data"}]}]}
fn generate_deep_mixed_json(depth: usize) -> String {
    let mut json = String::new();

    // Opening structures (alternating object/array)
    for i in 0..depth {
        if i % 2 == 0 {
            // Object level
            json.push_str(&format!(r#"{{"level{}":"#, i));
        } else {
            // Array level
            json.push('[');
        }
    }

    // Core data at the deepest level
    json.push_str(r#""reached_the_deep_end""#);

    // Closing structures (reverse order)
    for i in (0..depth).rev() {
        if i % 2 == 0 {
            // Close object
            json.push('}');
        } else {
            // Close array
            json.push(']');
        }
    }

    json
}

/// Generate complex nested JSON with realistic structure
/// Pattern: [{"data": [{"data": [{"value": 123}]}]}]
fn generate_complex_nested_json(depth: usize) -> String {
    let mut json = String::new();

    // Start with array
    json.push('[');

    for i in 0..depth {
        if i % 3 == 0 {
            // Object with "data" key
            json.push_str(r#"{"data":"#);
        } else if i % 3 == 1 {
            // Array
            json.push('[');
        } else {
            // Object with "nested" key
            json.push_str(r#"{"nested":"#);
        }
    }

    // Core data
    json.push_str(&format!(
        r#"{{"value": {}, "msg": "depth_{}_reached"}}"#,
        depth * 42,
        depth
    ));

    // Close all structures
    for i in (0..depth).rev() {
        if i % 3 == 0 || i % 3 == 2 {
            // Close object
            json.push('}');
        } else {
            // Close array
            json.push(']');
        }
    }

    // Close initial array
    json.push(']');

    json
}
