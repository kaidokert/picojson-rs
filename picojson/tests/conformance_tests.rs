#[cfg(feature = "remote-tests")]
mod conformance_tests {
    use picojson::{Event, ParseError, PullParser, SliceParser};
    use std::fs;
    use std::path::Path;

    const CONFORMANCE_DIR: &str = "conformance_tests/test_parsing";

    fn get_test_files() -> Vec<(String, String)> {
        let test_path = Path::new(CONFORMANCE_DIR);
        if !test_path.exists() {
            panic!("Conformance test files not found. Run: cargo run --bin download --features remote-tests");
        }

        let mut files = Vec::new();
        for entry in fs::read_dir(test_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().unwrap_or_default() == "json" {
                let filename = path.file_name().unwrap().to_string_lossy().to_string();

                // Some test files contain invalid UTF-8 by design
                match fs::read_to_string(&path) {
                    Ok(content) => files.push((filename, content)),
                    Err(_) => {
                        // Skip invalid UTF-8 files for now
                        println!("Skipping {} (invalid UTF-8)", filename);
                    }
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    fn run_parser_test(json_content: &str) -> Result<usize, ParseError> {
        let mut buffer = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(json_content, &mut buffer);
        let mut event_count = 0;

        loop {
            match parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(_event) => event_count += 1,
                Err(e) => return Err(e),
            }
        }
        Ok(event_count)
    }

    #[test]
    fn test_conformance_should_pass() {
        let test_files = get_test_files();
        let mut passed = 0;
        let mut failed = 0;

        for (filename, content) in test_files {
            // Test files starting with 'y_' should pass
            if filename.starts_with("y_") {
                match run_parser_test(&content) {
                    Ok(event_count) => {
                        passed += 1;
                        println!("✓ {} ({} events)", filename, event_count);
                    }
                    Err(e) => {
                        failed += 1;
                        println!("✗ {} - Error: {:?}", filename, e);
                    }
                }
            }
        }

        println!("Should pass: {} passed, {} failed", passed, failed);

        assert_eq!(failed, 0, "Some tests that should pass are failing");
    }

    #[test]
    fn test_conformance_should_fail() {
        let test_files = get_test_files();
        let mut correctly_failed = 0;
        let mut incorrectly_passed = 0;

        for (filename, content) in test_files {
            // Test files starting with 'n_' should fail
            if filename.starts_with("n_") {
                match run_parser_test(&content) {
                    Ok(event_count) => {
                        incorrectly_passed += 1;
                        println!(
                            "✗ {} - Should have failed but passed ({} events)",
                            filename, event_count
                        );
                    }
                    Err(_e) => {
                        correctly_failed += 1;
                        println!("✓ {} - Correctly failed", filename);
                    }
                }
            }
        }

        println!(
            "Should fail: {} correctly failed, {} incorrectly passed",
            correctly_failed, incorrectly_passed
        );

        assert_eq!(
            incorrectly_passed, 0,
            "Some tests that should fail are passing"
        );
    }

    #[test]
    fn test_conformance_implementation_dependent() {
        let test_files = get_test_files();
        let mut passed = 0;
        let mut failed = 0;

        for (filename, content) in test_files {
            // Test files starting with 'i_' are implementation dependent
            if filename.starts_with("i_") {
                match run_parser_test(&content) {
                    Ok(event_count) => {
                        passed += 1;
                        println!(
                            "✓ {} - Passed (implementation choice) ({} events)",
                            filename, event_count
                        );
                    }
                    Err(e) => {
                        failed += 1;
                        println!("✗ {} - Failed (implementation choice): {:?}", filename, e);
                    }
                }
            }
        }

        println!(
            "Implementation dependent: {} passed, {} failed",
            passed, failed
        );

        // These tests are implementation dependent, so we just report results
    }

    #[test]
    fn test_conformance_summary() {
        let test_files = get_test_files();
        let mut y_count = 0;
        let mut n_count = 0;
        let mut i_count = 0;

        for (filename, _) in test_files {
            if filename.starts_with("y_") {
                y_count += 1;
            } else if filename.starts_with("n_") {
                n_count += 1;
            } else if filename.starts_with("i_") {
                i_count += 1;
            }
        }

        println!("Conformance test summary:");
        println!("  {} tests should pass (y_*)", y_count);
        println!("  {} tests should fail (n_*)", n_count);
        println!("  {} tests are implementation dependent (i_*)", i_count);
        println!("  {} total tests", y_count + n_count + i_count);
    }
}
