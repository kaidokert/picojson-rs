// SPDX-License-Identifier: Apache-2.0

//! JSON_checker test suite integration
//!
//! This module runs the classic JSON_checker test suite from json.org
//! to validate parser compliance with JSON specification edge cases.
//!
//! The test suite contains:
//! - 3 pass*.json files that should parse successfully
//! - 33 fail*.json files that should error during parsing
//!
//! This provides complementary validation to JSONTestSuite with focused
//! testing of specific JSON specification violations.

#[cfg(feature = "remote-tests")]
mod json_checker_tests {
    use picojson::{
        ChunkReader, DefaultConfig, Event, ParseError, PullParser, PushParseError, PushParser,
        PushParserHandler, SliceParser, StreamParser,
    };
    use std::fs;
    use std::path::Path;

    fn run_parser_test(json_content: &str) -> Result<usize, ParseError> {
        let mut buffer = [0u8; 2048]; // Larger buffer for pass1.json
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

    // Test handler for PushParser conformance tests
    struct ConformanceTestHandler {
        event_count: usize,
    }

    impl<'a, 'b> PushParserHandler<'a, 'b, ParseError> for ConformanceTestHandler {
        fn handle_event(&mut self, _event: Event<'a, 'b>) -> Result<(), ParseError> {
            self.event_count += 1;
            Ok(())
        }
    }

    fn run_push_parser_test(json_content: &str) -> Result<usize, ParseError> {
        let mut buffer = [0u8; 2048]; // Larger buffer for pass1.json
        let handler = ConformanceTestHandler { event_count: 0 };
        let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

        let to_parse_error = |e: PushParseError<ParseError>| match e {
            PushParseError::Parse(parse_err) => parse_err,
            PushParseError::Handler(handler_err) => handler_err,
        };

        parser
            .write(json_content.as_bytes())
            .map_err(to_parse_error)?;

        let handler = parser.finish::<ParseError>().map_err(to_parse_error)?;
        Ok(handler.event_count)
    }

    fn run_stream_parser_test(json_content: &str) -> Result<usize, ParseError> {
        let reader = ChunkReader::full_slice(json_content.as_bytes());
        let mut buffer = [0u8; 2048]; // Larger buffer for pass1.json
        let mut parser = StreamParser::<_, DefaultConfig>::new(reader, &mut buffer);
        let mut event_count = 0;

        loop {
            match parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(_event) => {
                    event_count += 1;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(event_count)
    }

    fn load_test_file(filename: &str) -> String {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let path = Path::new(&manifest_dir)
            .join("tests/data/json_checker")
            .join(filename);
        fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Failed to read test file: {}", filename))
    }

    mod should_pass {
        use super::*;

        #[test]
        fn test_pass1_comprehensive() {
            let content = load_test_file("pass1.json");
            let result = run_parser_test(&content);
            assert!(
                result.is_ok(),
                "pass1.json should parse successfully but failed: {:?}",
                result.err()
            );

            // pass1.json is a comprehensive test with many JSON features
            let event_count = result.unwrap();
            assert!(
                event_count > 50,
                "pass1.json should generate substantial events, got: {}",
                event_count
            );
        }

        #[test]
        fn test_pass2_deep_nesting() {
            let content = load_test_file("pass2.json");
            let result = run_parser_test(&content);
            assert!(
                result.is_ok(),
                "pass2.json (deep nesting) should parse successfully but failed: {:?}",
                result.err()
            );
        }

        #[test]
        fn test_pass3_simple_object() {
            let content = load_test_file("pass3.json");
            let result = run_parser_test(&content);
            assert!(
                result.is_ok(),
                "pass3.json (simple object) should parse successfully but failed: {:?}",
                result.err()
            );
        }

        // PushParser conformance tests
        #[test]
        fn test_push_parser_pass1_comprehensive() {
            let content = load_test_file("pass1.json");
            let result = run_push_parser_test(&content);
            assert!(
                result.is_ok(),
                "PushParser: pass1.json should parse successfully but failed: {:?}",
                result.err()
            );

            // pass1.json is a comprehensive test with many JSON features
            let event_count = result.unwrap();
            assert!(
                event_count > 50,
                "PushParser: pass1.json should generate substantial events, got: {}",
                event_count
            );
        }

        #[test]
        fn test_push_parser_pass2_deep_nesting() {
            let content = load_test_file("pass2.json");
            let result = run_push_parser_test(&content);
            assert!(
                result.is_ok(),
                "PushParser: pass2.json (deep nesting) should parse successfully but failed: {:?}",
                result.err()
            );
        }

        #[test]
        fn test_push_parser_pass3_simple_object() {
            let content = load_test_file("pass3.json");
            let result = run_push_parser_test(&content);
            assert!(
                result.is_ok(),
                "PushParser: pass3.json (simple object) should parse successfully but failed: {:?}",
                result.err()
            );
        }

        // StreamParser conformance tests with logging
        #[test]
        fn test_stream_parser_pass1_comprehensive() {
            let content = load_test_file("pass1.json");
            let result = run_stream_parser_test(&content);
            assert!(
                result.is_ok(),
                "StreamParser: pass1.json should parse successfully but failed: {:?}",
                result.err()
            );

            // pass1.json is a comprehensive test with many JSON features
            let event_count = result.unwrap();
            assert!(
                event_count > 50,
                "StreamParser: pass1.json should generate substantial events, got: {}",
                event_count
            );
        }

        #[test]
        fn test_stream_parser_pass2_deep_nesting() {
            let content = load_test_file("pass2.json");
            let result = run_stream_parser_test(&content);
            assert!(
                result.is_ok(),
                "StreamParser: pass2.json (deep nesting) should parse successfully but failed: {:?}",
                result.err()
            );
        }

        #[test]
        fn test_stream_parser_pass3_simple_object() {
            let content = load_test_file("pass3.json");
            let result = run_stream_parser_test(&content);
            assert!(
                result.is_ok(),
                "StreamParser: pass3.json (simple object) should parse successfully but failed: {:?}",
                result.err()
            );
        }
    }

    // Indices of fail*.json files that should fail to parse (excluding known deviations)
    const EXPECTED_FAIL_INDICES: &[u32] = &[
        2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26, 27,
        28, 29, 30, 31, 32, 33,
    ];

    mod should_fail {
        use super::*;

        macro_rules! generate_fail_tests {
            ($($num:expr),*) => {
                $(
                    paste::paste! {
                        #[test]
                        fn [<test_fail $num>]() {
                            let content = load_test_file(&format!("fail{}.json", $num));
                            let result = run_parser_test(&content);
                            assert!(
                                result.is_err(),
                                "fail{}.json should fail to parse but succeeded with {} events. Content: {:?}",
                                $num,
                                result.unwrap_or(0),
                                content
                            );
                        }
                    }
                )*
            };
        }

        // Generate individual test cases for the 31 fail*.json files that are expected to fail
        generate_fail_tests!(
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26,
            27, 28, 29, 30, 31, 32, 33
        );

        macro_rules! generate_push_parser_fail_tests {
            ($($num:expr),*) => {
                $(
                    paste::paste! {
                        #[test]
                        fn [<test_push_parser_fail $num>]() {
                            let content = load_test_file(&format!("fail{}.json", $num));
                            let result = run_push_parser_test(&content);
                            assert!(
                                result.is_err(),
                                "PushParser: fail{}.json should fail to parse but succeeded with {} events. Content: {:?}",
                                $num,
                                result.unwrap_or(0),
                                content
                            );
                        }
                    }
                )*
            };
        }

        // Generate PushParser test cases for the same 31 fail*.json files
        generate_push_parser_fail_tests!(
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26,
            27, 28, 29, 30, 31, 32, 33
        );
    }

    mod known_deviations {
        use super::*;

        #[test]
        fn test_fail1_root_string_allowed() {
            let content = load_test_file("fail1.json");
            let result = run_parser_test(&content);
            assert!(
                result.is_ok(),
                "fail1.json is expected to pass because modern JSON (RFC 7159) allows scalar root values."
            );
        }

        #[test]
        fn test_fail18_deep_nesting_supported() {
            let content = load_test_file("fail18.json");
            let result = run_parser_test(&content);
            assert!(
                result.is_ok(),
                "fail18.json is expected to pass because the non-recursive parser handles deep nesting."
            );
        }

        // PushParser known deviations - should match SliceParser behavior
        #[test]
        fn test_push_parser_fail1_root_string_allowed() {
            let content = load_test_file("fail1.json");
            let result = run_push_parser_test(&content);
            assert!(
                result.is_ok(),
                "PushParser: fail1.json is expected to pass because modern JSON (RFC 7159) allows scalar root values."
            );
        }

        #[test]
        fn test_push_parser_fail18_deep_nesting_supported() {
            let content = load_test_file("fail18.json");
            let result = run_push_parser_test(&content);
            assert!(
                result.is_ok(),
                "PushParser: fail18.json is expected to pass because the non-recursive parser handles deep nesting."
            );
        }
    }

    #[test]
    fn test_comprehensive_suite() {
        let mut pass_count = 0;
        let mut fail_count = 0;
        let mut deviation_count = 0;

        // Test all pass files
        for i in 1..=3 {
            let filename = format!("pass{}.json", i);
            let content = load_test_file(&filename);
            if run_parser_test(&content).is_ok() {
                pass_count += 1;
            } else {
                panic!("Expected {} to pass but it failed.", filename);
            }
        }

        // Test all fail files (excluding known deviations)
        for i in EXPECTED_FAIL_INDICES {
            let filename = format!("fail{}.json", i);
            let content = load_test_file(&filename);
            if run_parser_test(&content).is_err() {
                fail_count += 1;
            } else {
                panic!("Expected {} to fail but it passed.", filename);
            }
        }

        // Test known deviations
        let deviation_indices = [1, 18];
        for i in &deviation_indices {
            let filename = format!("fail{}.json", i);
            let content = load_test_file(&filename);
            if run_parser_test(&content).is_ok() {
                deviation_count += 1;
            } else {
                panic!("Expected deviation {} to pass but it failed.", filename);
            }
        }

        println!("JSON_checker test suite results:");
        println!("  Pass tests: {}/3 ✓", pass_count);
        println!(
            "  Fail tests: {}/{} ✓",
            fail_count,
            EXPECTED_FAIL_INDICES.len()
        );
        println!("  Known deviations (passing): {}/2 ✓", deviation_count);
        println!(
            "  Total: {}/{} tests behaved as expected",
            pass_count + fail_count + deviation_count,
            3 + EXPECTED_FAIL_INDICES.len() + 2
        );

        assert_eq!(pass_count, 3, "All pass tests should succeed");
        assert_eq!(
            fail_count,
            EXPECTED_FAIL_INDICES.len(),
            "All fail tests should error"
        );
        assert_eq!(deviation_count, 2, "All deviation tests should succeed");
    }
}
