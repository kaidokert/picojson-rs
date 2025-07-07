// SPDX-License-Identifier: Apache-2.0

use picojson::{ChunkReader, Event, JsonNumber, NumberResult, PullParser, Reader, StreamParser};

/// Reader that provides data in chunks of varying sizes, following a repeating pattern.
struct VariableChunkReader<'a> {
    data: &'a [u8],
    pos: usize,
    chunk_pattern: &'a [usize],
    pattern_idx: usize,
}

impl<'a> VariableChunkReader<'a> {
    fn new(data: &'a [u8], chunk_pattern: &'a [usize]) -> Self {
        Self {
            data,
            pos: 0,
            chunk_pattern,
            pattern_idx: 0,
        }
    }
}

impl<'a> Reader for VariableChunkReader<'a> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }

        let chunk_size = self.chunk_pattern[self.pattern_idx].max(1);
        self.pattern_idx = (self.pattern_idx + 1) % self.chunk_pattern.len();

        let to_copy = remaining.min(buf.len()).min(chunk_size);
        buf[..to_copy].copy_from_slice(&self.data[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

// --- Test Scenarios ---

struct TestScenario<'a> {
    name: &'static str,
    json: &'a [u8],
    expected_events: Vec<Event<'a, 'a>>,
    min_buffer_size: usize,
}

/// Gallery of stressful JSON documents
fn get_test_scenarios<'a>() -> Vec<TestScenario<'a>> {
    vec![
        TestScenario {
            name: "Original",
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
            min_buffer_size: 6, // Longest token is "world" or "count" (5) + quotes
        },
        TestScenario {
            name: "Empty Strings",
            json: br#"{"":""}"#,
            expected_events: vec![
                Event::StartObject,
                Event::Key("".into()),
                Event::String("".into()),
                Event::EndObject,
                Event::EndDocument,
            ],
            min_buffer_size: 1, // Empty strings work with minimal buffer
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
            min_buffer_size: 27, // Empirically determined minimum
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
            min_buffer_size: 31, // Empirically determined minimum
        },
        TestScenario {
            name: "Deeply Nested",
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
            min_buffer_size: 3, // Empirically determined minimum
        },
        TestScenario {
            name: "Mixed Escapes",
            json: br#"["a\nb\t\"\\c\u1234d"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String(picojson::String::Unescaped("a\nb\t\"\\cሴd")), // \u1234 = ሴ (Ethiopian character), Unescaped variant
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 21, // Escape reserve removed: much smaller buffer needed
        },
        TestScenario {
            name: "String ending with escape",
            json: br#"["hello\\"]"#,
            expected_events: vec![
                Event::StartArray,
                Event::String(picojson::String::Unescaped("hello\\")), // Contains escape, so Unescaped
                Event::EndArray,
                Event::EndDocument,
            ],
            min_buffer_size: 10, // Escape reserve removed: works with just 10 bytes!
        },
    ]
}

/// Core test function that validates parsing with given buffer and chunk sizes
fn test_parsing_with_config(
    scenario: &TestScenario,
    buffer_size: usize,
    reader: impl Reader<Error = ()>,
) -> Result<(), String> {
    let mut buffer = vec![0u8; buffer_size];
    let mut parser = StreamParser::new(reader, &mut buffer);

    for (i, expected_event) in scenario.expected_events.iter().enumerate() {
        match parser.next_event() {
            Ok(event) => {
                if &event != expected_event {
                    return Err(format!(
                        "Mismatch at event {}: expected {:?}, got {:?}",
                        i, expected_event, event
                    ));
                }
            }
            Err(e) => {
                return Err(format!("Parser error at event {}: {:?}", i, e));
            }
        }
    }

    Ok(())
}

/// Determine if a given buffer size should succeed or fail
fn should_succeed(buffer_size: usize, min_buffer_size: usize) -> bool {
    // This is still an estimation, but a more conservative one.
    // The parser needs to sometimes hold a token and its surrounding delimiters.
    buffer_size >= min_buffer_size
}

#[test]
fn test_stress_buffer_sizes_with_full_reads() {
    let scenarios = get_test_scenarios();
    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        let chunk_size = scenario.json.len() + 1; // Read entire JSON at once

        for buffer_size in 1..=40 {
            let reader = ChunkReader::new(scenario.json, chunk_size);
            let result = test_parsing_with_config(scenario, buffer_size, reader);
            let expected_success = should_succeed(buffer_size, scenario.min_buffer_size);

            match (result.is_ok(), expected_success) {
                (true, true) => {
                    println!("✅ [B={}] SUCCESS (expected)", buffer_size);
                }
                (false, false) => {
                    println!("✅ [B={}] FAIL (expected)", buffer_size);
                }
                (true, false) => {
                    panic!("❌ [B={}] Unexpected SUCCESS", buffer_size);
                }
                (false, true) => {
                    panic!(
                        "❌ [B={}] Unexpected FAILURE - {}",
                        buffer_size,
                        result.unwrap_err()
                    );
                }
            }
        }
    }
}

#[test]
fn test_stress_chunk_sizes_with_adequate_buffer() {
    let scenarios = get_test_scenarios();
    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        let buffer_size = scenario.min_buffer_size + 5; // Adequate size

        let chunk_sizes = [1, 2, 3, 5, 8, 10, 15, 28, 50];

        for &chunk_size in &chunk_sizes {
            let reader = ChunkReader::new(scenario.json, chunk_size);
            let result = test_parsing_with_config(scenario, buffer_size, reader);

            match result {
                Ok(()) => {
                    println!("✅ [C={}] SUCCESS", chunk_size);
                }
                Err(e) => {
                    panic!("❌ [C={}] Unexpected FAILURE - {}", chunk_size, e);
                }
            }
        }
    }
}

#[test]
fn test_stress_matrix_critical_sizes() {
    let scenarios = get_test_scenarios();
    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        let critical_buffer_sizes: Vec<usize> = (1..=scenario.min_buffer_size + 3).collect();
        let chunk_sizes = [1, 2, 5, 10, 28];

        for &buffer_size in &critical_buffer_sizes {
            for &chunk_size in &chunk_sizes {
                let reader = ChunkReader::new(scenario.json, chunk_size);
                let result = test_parsing_with_config(scenario, buffer_size, reader);
                let expected_success = should_succeed(buffer_size, scenario.min_buffer_size);

                match (result.is_ok(), expected_success) {
                    (true, true) => {
                        println!(
                            "✅ [B={}, C={}] SUCCESS (expected)",
                            buffer_size, chunk_size
                        );
                    }
                    (false, false) => {
                        println!("✅ [B={}, C={}] FAIL (expected)", buffer_size, chunk_size);
                    }
                    (true, false) => {
                        panic!(
                            "❌ [B={}, C={}] Unexpected SUCCESS",
                            buffer_size, chunk_size
                        );
                    }
                    (false, true) => {
                        panic!(
                            "❌ [B={}, C={}] Unexpected FAILURE - {}",
                            buffer_size,
                            chunk_size,
                            result.unwrap_err()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_stress_variable_read_sizes() {
    let scenarios = get_test_scenarios();
    let patterns: &[&[usize]] = &[&[1, 5, 2], &[7, 1, 1, 10], &[1]];

    for scenario in &scenarios {
        println!("--- Testing Scenario: {} ---", scenario.name);
        for &buffer_size in &[scenario.min_buffer_size, scenario.min_buffer_size + 10] {
            for &pattern in patterns {
                let reader = VariableChunkReader::new(scenario.json, pattern);
                let result = test_parsing_with_config(scenario, buffer_size, reader);
                if let Err(e) = result {
                    panic!(
                        "❌ [B={}, P={:?}] Unexpected FAILURE - {}",
                        buffer_size, pattern, e
                    );
                } else {
                    println!("✅ [B={}, P={:?}] SUCCESS", buffer_size, pattern);
                }
            }
        }
    }
}
