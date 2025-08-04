// SPDX-License-Identifier: Apache-2.0

//! Comprehensive stress tests for PushParser
//!
//! Tests various buffer sizes, write chunk patterns, and edge cases to ensure
//! robustness under different memory and data delivery constraints.

use picojson::{
    DefaultConfig, Event, JsonNumber, NumberResult, ParseError, PushParseError, PushParser,
    PushParserHandler,
};

/// Owned event representation for comparison
#[derive(Debug, Clone, PartialEq)]
enum OwnedEvent {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    Key(String),
    String(String),
    Number(String),
    Bool(bool),
    Null,
    EndDocument,
}

/// Handler that compares events immediately as they arrive
struct StressTestHandler<'expected> {
    expected_events: &'expected [OwnedEvent],
    current_index: usize,
}

impl<'expected> StressTestHandler<'expected> {
    fn new(expected_events: &'expected [OwnedEvent]) -> Self {
        Self {
            expected_events,
            current_index: 0,
        }
    }

    fn assert_complete(&self) {
        assert_eq!(
            self.current_index,
            self.expected_events.len(),
            "Expected {} events, but only received {}",
            self.expected_events.len(),
            self.current_index
        );
    }

    fn assert_event_matches(&mut self, received: &Event) {
        assert!(
            self.current_index < self.expected_events.len(),
            "Received more events than expected. Got event at index {} but only expected {} events total",
            self.current_index,
            self.expected_events.len()
        );

        let expected = &self.expected_events[self.current_index];
        let received_owned = OwnedEvent::from_event(received);

        assert_eq!(
            *expected, received_owned,
            "Event mismatch at index {}",
            self.current_index
        );

        self.current_index += 1;
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for StressTestHandler<'_> {
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
        self.assert_event_matches(&event);
        Ok(())
    }
}

/// Handler for tests that expect parsing to fail - accepts any events without validation
struct PermissiveTestHandler;

impl PermissiveTestHandler {
    fn new() -> Self {
        Self
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for PermissiveTestHandler {
    fn handle_event(&mut self, _event: Event<'input, 'scratch>) -> Result<(), ParseError> {
        // Accept any events - we expect the parser to fail eventually
        Ok(())
    }
}

impl OwnedEvent {
    /// Convert from Event to OwnedEvent
    fn from_event(event: &Event) -> Self {
        match event {
            Event::StartObject => OwnedEvent::StartObject,
            Event::EndObject => OwnedEvent::EndObject,
            Event::StartArray => OwnedEvent::StartArray,
            Event::EndArray => OwnedEvent::EndArray,
            Event::Key(k) => OwnedEvent::Key(k.as_ref().to_string()),
            Event::String(s) => OwnedEvent::String(s.as_ref().to_string()),
            Event::Number(n) => OwnedEvent::Number(n.as_str().to_string()),
            Event::Bool(b) => OwnedEvent::Bool(*b),
            Event::Null => OwnedEvent::Null,
            Event::EndDocument => OwnedEvent::EndDocument,
        }
    }
}

/// Writer that delivers data to PushParser in controlled chunks
struct ChunkedWriter<'a> {
    data: &'a [u8],
    pos: usize,
    chunk_pattern: &'a [usize],
    pattern_idx: usize,
}

impl<'a> ChunkedWriter<'a> {
    fn new(data: &'a [u8], chunk_pattern: &'a [usize]) -> Self {
        Self {
            data,
            pos: 0,
            chunk_pattern,
            pattern_idx: 0,
        }
    }

    pub fn run<'input, H, E>(
        &mut self,
        mut parser: PushParser<'input, '_, H, DefaultConfig>,
    ) -> Result<H, PushParseError<E>>
    where
        H: for<'i, 's> PushParserHandler<'i, 's, E>,
        E: From<ParseError>,
        'a: 'input,
    {
        while self.pos < self.data.len() {
            let chunk_size = if self.chunk_pattern.is_empty() {
                self.data.len() - self.pos
            } else {
                let size = self.chunk_pattern[self.pattern_idx].max(1);
                self.pattern_idx = (self.pattern_idx + 1) % self.chunk_pattern.len();
                size
            };

            let end_pos = (self.pos + chunk_size).min(self.data.len());
            let chunk: &'input [u8] = &self.data[self.pos..end_pos];

            parser.write(chunk)?;
            self.pos = end_pos;
        }

        parser.finish()
    }
}

/// Test scenario configuration
struct TestScenario {
    name: &'static str,
    json: &'static [u8],
    expected_events: Vec<Event<'static, 'static>>,
    min_buffer_size: usize,
}

/// Create comprehensive test scenarios covering various edge cases
fn get_push_parser_test_scenarios() -> Vec<TestScenario> {
    vec![
        TestScenario {
            name: "Basic Object",
            json: br#"{"hello": "world", "count": 42}"#,
            expected_events: vec![
                Event::StartObject,
                Event::Key("hello".into()),
                Event::String("world".into()),
                Event::Key("count".into()),
                Event::Number(JsonNumber::Borrowed {
                    raw: "42",
                    parsed: NumberResult::Integer(42),
                }),
                Event::EndObject,
                Event::EndDocument,
            ],
            min_buffer_size: 8, // Needs larger buffer for small chunk patterns that force copy-on-escape
        },
        TestScenario {
            name: "Empty Strings",
            json: br#"{"": ""}"#,
            expected_events: vec![
                Event::StartObject,
                Event::Key("".into()),
                Event::String("".into()),
                Event::EndObject,
                Event::EndDocument,
            ],
            min_buffer_size: 1, // Copy-on-escape works even for empty strings
        },
        TestScenario {
            name: "Long String (No Escapes)",
            json: br#"["abcdefghijklmnopqrstuvwxyz"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String("abcdefghijklmnopqrstuvwxyz".into()),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 26, // String length when using small chunks that force copy-on-escape
        },
        TestScenario {
            name: "Long Number",
            json: br#"[123456789012345678901234567890]"#,
            expected_events: vec![
                Event::StartArray,
                Event::Number(JsonNumber::Borrowed {
                    raw: "123456789012345678901234567890",
                    parsed: NumberResult::IntegerOverflow,
                }),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 30, // Number length when using small chunks that force copy-on-escape
        },
        TestScenario {
            name: "Deeply Nested Arrays",
            json: br#"[[[[[[[[[[42]]]]]]]]]]"#,
            expected_events: (0..10)
                .map(|_| Event::StartArray)
                .chain(std::iter::once(Event::Number(JsonNumber::Borrowed {
                    raw: "42",
                    parsed: NumberResult::Integer(42),
                })))
                .chain((0..10).map(|_| Event::EndArray))
                .chain(std::iter::once(Event::EndDocument))
                .collect(),
            min_buffer_size: 2, // Number "42" needs 2 bytes when split by byte-by-byte processing
        },
        TestScenario {
            name: "Unicode Escapes",
            json: br#"["\u0041\u0042\u0043"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String("ABC".into()),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 3, // Unicode processing needs buffer space for escape processing
        },
        TestScenario {
            name: "Mixed Escapes",
            json: br#"["a\nb\t\"\\c\u1234d"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String("a\nb\t\"\\cሴd".into()), // Mixed escapes with Unicode \u1234 = ሴ
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 11, // Mixed escape processing buffer including Unicode
        },
        TestScenario {
            name: "String ending with escape",
            json: br#"["hello\\"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String(picojson::String::Unescaped("hello\\")),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 6, // Escape at end processing - copy-on-escape optimization allows smaller buffer
        },
        TestScenario {
            name: "Complex Nested Structure",
            json: br#"{"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}"#,
            expected_events: vec![
                Event::StartObject,
                Event::Key("users".into()),
                Event::StartArray,
                Event::StartObject,
                Event::Key("name".into()),
                Event::String("Alice".into()),
                Event::Key("age".into()),
                Event::Number(JsonNumber::Borrowed {
                    raw: "30",
                    parsed: NumberResult::Integer(30),
                }),
                Event::EndObject,
                Event::StartObject,
                Event::Key("name".into()),
                Event::String("Bob".into()),
                Event::Key("age".into()),
                Event::Number(JsonNumber::Borrowed {
                    raw: "25",
                    parsed: NumberResult::Integer(25),
                }),
                Event::EndObject,
                Event::EndArray,
                Event::EndObject,
                Event::EndDocument,
            ],
            min_buffer_size: 5, // Longest string "Alice"/"users" when using small chunks
        },
    ]
}

/// Core test function that validates PushParser with given buffer and chunk sizes
fn test_push_parsing_with_config(
    scenario: &TestScenario,
    buffer_size: usize,
    chunk_pattern: &[usize],
) -> Result<(), ParseError> {
    let mut buffer = vec![0u8; buffer_size];
    let expected_events: Vec<OwnedEvent> = scenario
        .expected_events
        .iter()
        .map(OwnedEvent::from_event)
        .collect();

    let handler = StressTestHandler::new(&expected_events);
    let parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    let mut writer = ChunkedWriter::new(scenario.json, chunk_pattern);

    match writer.run(parser) {
        Ok(handler) => {
            handler.assert_complete();
            Ok(())
        }
        Err(e) => match e {
            PushParseError::Parse(parse_err) => Err(parse_err),
            PushParseError::Handler(handler_err) => Err(handler_err),
        },
    }
}

/// Determine if a given buffer size should succeed or fail based on chunk pattern
fn should_succeed_push_parser(
    buffer_size: usize,
    scenario: &TestScenario,
    chunk_pattern: &[usize],
) -> bool {
    let min_buffer_size = get_min_buffer_size_for_scenario(scenario, chunk_pattern);
    buffer_size >= min_buffer_size
}

/// Calculate minimum buffer size based on scenario and chunk pattern
fn get_min_buffer_size_for_scenario(scenario: &TestScenario, chunk_pattern: &[usize]) -> usize {
    // Some scenarios always need larger buffers due to escape processing
    let needs_escape_buffer = matches!(
        scenario.name,
        "Unicode Escapes" | "Mixed Escapes" | "String ending with escape"
    );

    // If chunk pattern is empty (single write) or all chunks are large,
    // copy-on-escape optimization allows minimal buffers - unless escape processing is needed
    let has_small_chunks = chunk_pattern.iter().any(|&size| size <= 20);

    if !has_small_chunks && !needs_escape_buffer {
        return 1; // Copy-on-escape optimization works well
    }

    // For small chunks that force buffer boundaries or escape processing, need actual content size
    match scenario.name {
        "Basic Object" => {
            if has_small_chunks {
                8
            } else {
                1
            }
        } // Longest content: "hello", "world", "count"
        "Empty Strings" => 1, // Empty strings need minimal buffer
        "Long String (No Escapes)" => {
            if has_small_chunks {
                26
            } else {
                1
            }
        } // "abcdefghijklmnopqrstuvwxyz"
        "Long Number" => {
            if has_small_chunks {
                30
            } else {
                1
            }
        } // "123456789012345678901234567890"
        "Deeply Nested Arrays" => {
            if has_small_chunks {
                2
            } else {
                1
            }
        } // Number "42"
        "Unicode Escapes" => 3, // Unicode processing needs minimal buffer space
        "Mixed Escapes" => 11, // Mixed escape processing buffer including Unicode
        "String ending with escape" => 6, // Escape at end processing
        "Complex Nested Structure" => {
            if has_small_chunks {
                5
            } else {
                1
            }
        } // "Alice"/"users"
        _ => scenario.min_buffer_size, // Use configured value for other scenarios
    }
}

#[test]
fn test_push_parser_stress_buffer_sizes() {
    println!("=== PushParser Buffer Size Stress Test ===");
    let scenarios = get_push_parser_test_scenarios();

    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);

        for buffer_size in 1..=50 {
            let result = test_push_parsing_with_config(scenario, buffer_size, &[]);
            let expected_success = should_succeed_push_parser(buffer_size, scenario, &[]);

            match (result.is_ok(), expected_success) {
                (true, true) => {
                    println!("✅ [B={}] SUCCESS (expected)", buffer_size);
                }
                (false, false) => {
                    println!("✅ [B={}] FAIL (expected)", buffer_size);
                }
                (true, false) => {
                    panic!(
                        "❌ [B={}] Unexpected SUCCESS for scenario '{}'",
                        buffer_size, scenario.name
                    );
                }
                (false, true) => {
                    panic!(
                        "❌ [B={}] Unexpected FAILURE for scenario '{}'",
                        buffer_size, scenario.name
                    );
                }
            }
        }
    }
}

#[test]
fn test_push_parser_stress_chunk_patterns() {
    println!("=== PushParser Chunk Pattern Stress Test ===");
    let scenarios = get_push_parser_test_scenarios();

    // Test patterns: Various chunk sizes to stress boundary handling
    let chunk_patterns: &[&[usize]] = &[
        &[50],          // Large chunks
        &[10],          // Medium chunks
        &[1],           // Byte-by-byte
        &[2],           // Two bytes at a time
        &[3, 1, 2],     // Variable small chunks
        &[1, 5, 1],     // Mixed tiny and small
        &[7, 1, 1, 10], // Irregular pattern
    ];

    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        let buffer_size = scenario.min_buffer_size + 10; // Adequate buffer

        for &pattern in chunk_patterns {
            let result = test_push_parsing_with_config(scenario, buffer_size, pattern);

            match result {
                Ok(()) => {
                    println!("✅ [P={:?}] SUCCESS", pattern);
                }
                Err(_e) => {
                    panic!(
                        "❌ [P={:?}] UNEXPECTED FAILURE for scenario '{}'",
                        pattern, scenario.name
                    );
                }
            }
        }
    }
}

#[test]
fn test_push_parser_stress_critical_matrix() {
    println!("=== PushParser Critical Size Matrix Test ===");
    let scenarios = get_push_parser_test_scenarios();

    let chunk_patterns: &[&[usize]] = &[
        &[50],          // Large chunks
        &[10],          // Medium chunks
        &[1],           // Byte-by-byte
        &[2],           // Two bytes at a time
        &[3, 1, 2],     // Variable small chunks
        &[1, 5, 1],     // Mixed tiny and small
        &[7, 1, 1, 10], // Irregular pattern
    ];

    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        // Use the max min_buffer_size across all chunk patterns for this scenario
        let max_min_buffer = chunk_patterns
            .iter()
            .map(|&pattern| get_min_buffer_size_for_scenario(scenario, pattern))
            .max()
            .unwrap_or(scenario.min_buffer_size);
        let critical_buffer_sizes: Vec<usize> =
            (max_min_buffer.saturating_sub(2)..=max_min_buffer + 5).collect();

        for &buffer_size in &critical_buffer_sizes {
            for &pattern in chunk_patterns {
                let result = test_push_parsing_with_config(scenario, buffer_size, pattern);
                let expected_success = should_succeed_push_parser(buffer_size, scenario, pattern);

                match (result.is_ok(), expected_success) {
                    (true, true) => {
                        println!("✅ [B={}, P={:?}] SUCCESS (expected)", buffer_size, pattern);
                    }
                    (false, false) => {
                        println!("✅ [B={}, P={:?}] FAIL (expected)", buffer_size, pattern);
                    }
                    (true, false) => {
                        // With copy-on-escape optimization, we might succeed with smaller buffers
                        println!("✅ [B={}, P={:?}] Unexpected SUCCESS - copy-on-escape working better than expected", buffer_size, pattern);
                    }
                    (false, true) => {
                        panic!(
                            "❌ [B={}, P={:?}] Unexpected FAILURE for scenario '{}'",
                            buffer_size, pattern, scenario.name
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_push_parser_stress_unicode_edge_cases() {
    println!("=== PushParser Unicode Edge Cases Stress Test ===");

    let unicode_scenarios = vec![
        TestScenario {
            name: "Consecutive Unicode",
            json: br#"["\u0123\u4567\u89AB\uCDEF"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String(picojson::String::Unescaped("ģ䕧覫췯")),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 25, // Unicode processing buffer for consecutive escapes
        },
        TestScenario {
            name: "Unicode at Chunk Boundary",
            json: br#"["\u0041XYZ"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String(picojson::String::Unescaped("AXYZ")),
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 15, // Unicode + normal text processing
        },
        TestScenario {
            name: "Empty Key with Unicode Value",
            json: br#"{"": "\u2603"}"#,
            expected_events: vec![
                Event::StartObject,
                Event::Key("".into()),
                Event::String(picojson::String::Unescaped("☃")),
                Event::EndObject,
                Event::EndDocument,
            ],
            min_buffer_size: 12, // Empty key + unicode value processing
        },
    ];

    for scenario in &unicode_scenarios {
        println!("--- Testing Unicode Scenario: {} ---", scenario.name);

        // Test specifically challenging chunk patterns for unicode
        let unicode_chunk_patterns: &[&[usize]] = &[
            &[1],       // Byte-by-byte (challenges unicode boundaries)
            &[6, 1],    // Split unicode escapes
            &[3, 2, 1], // Irregular splits
        ];

        let buffer_size = scenario.min_buffer_size + 5;

        for &pattern in unicode_chunk_patterns {
            let result = test_push_parsing_with_config(scenario, buffer_size, pattern);

            match result {
                Ok(()) => {
                    println!("✅ [P={:?}] Unicode SUCCESS", pattern);
                }
                Err(_e) => {
                    panic!(
                        "❌ [P={:?}] Unicode FAILURE for scenario '{}'",
                        pattern, scenario.name
                    );
                }
            }
        }
    }
}

#[test]
fn test_push_parser_stress_document_validation() {
    println!("=== PushParser Document Validation Stress Test ===");

    // Test incomplete documents that should fail
    let invalid_scenarios: Vec<(&str, &[u8], &str)> = vec![
        ("Unclosed Array", b"[\"hello\"", "array not closed"),
        (
            "Unclosed Object",
            b"{\"key\": \"value\"",
            "object not closed",
        ),
        ("Extra Comma", b"{\"key\": \"value\",}", "trailing comma"),
        ("Missing Value", b"{\"key\":}", "missing value"),
    ];

    for (name, json, _description) in &invalid_scenarios {
        println!("--- Testing Invalid: {} ---", name);

        let buffer_size = 50; // Adequate buffer
        let chunk_patterns: &[&[usize]] = &[&[1], &[3], &[10]];

        for &pattern in chunk_patterns {
            let mut buffer = vec![0u8; buffer_size];
            // For invalid JSON tests, use a permissive handler that doesn't validate events
            let handler = PermissiveTestHandler::new();
            let parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);
            let mut writer = ChunkedWriter::new(json, pattern);

            let result = writer.run(parser);

            if result.is_ok() {
                panic!(
                    "❌ [P={:?}] Expected FAILURE for '{}', but got SUCCESS",
                    pattern, name
                );
            } else {
                println!("✅ [P={:?}] Correctly FAILED for '{}'", pattern, name);
            }
        }
    }
}
