// Example demonstrating configurable number handling for embedded targets
// Shows both successful parsing and error scenarios based on input data.
//
// Try different configurations:
// cargo run --example no_float_demo --no-default-features                    # Basic no-float
// cargo run --example no_float_demo --features int32                         # 32-bit integers
// cargo run --example no_float_demo --features int32,float-truncate          # Truncate floats (shows both error and success paths)
// cargo run --example no_float_demo --features int32,float-error             # Error on floats (embedded-friendly)
// cargo run --example no_float_demo --features float                         # Full float support

use picojson::{Event, NumberResult, PullParser, SliceParser, String};

fn main() {
    // Full JSON with scientific notation
    let json_full = r#"{"integers": [1, 2, 3], "floats": [1.5, 2.7, 3.14], "scientific": [1e3, 2.5e-1, 1.23e+2], "mixed": [42, 1.618, 100]}"#;

    // Limited JSON without scientific notation (for truncate mode demonstration)
    let json_limited =
        r#"{"integers": [1, 2, 3], "floats": [1.5, 2.7, 3.14], "mixed": [42, 1.618, 100]}"#;

    println!("Parsing JSON with configurable number handling:");

    // Show configuration being used
    #[cfg(feature = "int32")]
    println!("Configuration: Using i32 integers (embedded-friendly)");
    #[cfg(feature = "int64")]
    println!("Configuration: Using i64 integers (full range)");

    #[cfg(feature = "float")]
    println!("Configuration: Float support enabled");
    #[cfg(all(not(feature = "float"), feature = "float-error"))]
    println!("Configuration: Error on floats (fail-fast for embedded)");
    #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
    println!("Configuration: Truncate floats to integers");
    #[cfg(all(
        not(feature = "float"),
        not(any(feature = "float-error", feature = "float-truncate"))
    ))]
    println!("Configuration: Float support disabled (raw strings only)");

    println!();

    // Determine which inputs to test based on configuration
    let test_cases = [
        ("Full JSON (with scientific notation)", json_full),
        ("Limited JSON (no scientific notation)", json_limited),
    ];

    // For float-truncate mode, test both to show error and success paths
    // For other modes, skip the second test if behavior would be identical
    let should_test_both = cfg!(all(not(feature = "float"), feature = "float-truncate"));

    for (i, (description, json)) in test_cases.iter().enumerate() {
        // Skip second test for non-truncate modes (behavior would be identical)
        if i == 1 && !should_test_both {
            break;
        }

        println!("=== {} ===", description);
        println!("Input: {}", json);
        println!();

        let mut scratch = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(json, &mut scratch);

        parse_and_display(&mut parser);

        if i == 0 && should_test_both {
            println!("\n--- Now testing without scientific notation ---\n");
        }
    }

    print_summary();
}

fn parse_and_display(parser: &mut SliceParser) {
    loop {
        match parser.next_event() {
            Ok(Event::Number(num)) => {
                println!("Number: raw='{}', parsed={:?}", num.as_str(), num.parsed());

                // Show behavior based on configuration
                match num.parsed() {
                    NumberResult::Integer(i) => println!("  → Integer: {}", i),
                    NumberResult::IntegerOverflow => {
                        println!("  → Integer overflow (use raw string): '{}'", num.as_str())
                    }
                    #[cfg(feature = "float")]
                    NumberResult::Float(f) => println!("  → Float: {}", f),
                    #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
                    NumberResult::FloatTruncated(i) => {
                        println!(
                            "  → Float truncated to integer: {} (from '{}')",
                            i,
                            num.as_str()
                        )
                    }
                    #[cfg(feature = "float-skip")]
                    NumberResult::FloatSkipped => {
                        println!("  → Float skipped (use raw string): '{}'", num.as_str())
                    }
                    #[cfg(not(feature = "float"))]
                    NumberResult::FloatDisabled => {
                        println!(
                            "  → Float disabled - raw string available: '{}'",
                            num.as_str()
                        );

                        // User could still parse manually if needed:
                        if let Ok(f) = num.as_str().parse::<f64>() {
                            println!("  → Manual parse as f64: {}", f);
                        }
                    }
                    // Handle variants that shouldn't be reachable in current configuration
                    _ => {
                        println!(
                            "  → Unexpected variant for current configuration: {:?}",
                            num.parsed()
                        );
                    }
                }
            }
            Ok(Event::Key(String::Borrowed(key))) => {
                println!("Key: '{}'", key);
            }
            Ok(Event::StartObject) => println!("StartObject"),
            Ok(Event::EndObject) => println!("EndObject"),
            Ok(Event::StartArray) => println!("StartArray"),
            Ok(Event::EndArray) => println!("EndArray"),
            Ok(Event::EndDocument) => {
                println!("EndDocument");
                break;
            }
            Ok(other) => println!("Other event: {:?}", other),
            Err(e) => {
                println!("Error: {:?}", e);
                break;
            }
        }
    }
}

fn print_summary() {
    println!("\n=== Summary ===");
    #[cfg(feature = "int32")]
    println!("- Using i32 integers (no 64-bit math routines needed)");
    #[cfg(feature = "int64")]
    println!("- Using i64 integers (full range)");

    #[cfg(feature = "float")]
    println!("- Float support enabled (f64 parsing)");
    #[cfg(all(not(feature = "float"), feature = "float-error"))]
    println!("- Error on floats (embedded fail-fast behavior)");
    #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
    println!("- Truncate floats to integers (simple decimals only, errors on scientific notation)");
    #[cfg(all(
        not(feature = "float"),
        not(any(feature = "float-error", feature = "float-truncate"))
    ))]
    println!("- Floats disabled (raw strings preserved for manual parsing)");

    println!("- Raw strings always preserved for exact precision");
    println!("- Zero heap allocations (no_std compatible)");

    println!("\nScientific notation handling:");
    #[cfg(feature = "float")]
    println!("- 1e3 = 1000, 2.5e-1 = 0.25, 1.23e+2 = 123 (full evaluation)");
    #[cfg(all(not(feature = "float"), feature = "float-error"))]
    println!("- All floats including scientific notation trigger FloatNotAllowed error");
    #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
    println!("- Scientific notation triggers InvalidNumber error (would require float math)");
    #[cfg(all(
        not(feature = "float"),
        not(any(feature = "float-error", feature = "float-truncate"))
    ))]
    println!("- Raw strings preserved: '1e3', '2.5e-1', '1.23e+2' (manual parsing available)");
}
