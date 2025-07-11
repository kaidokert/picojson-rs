// SPDX-License-Identifier: Apache-2.0

//! Shared surrogate pair tests for both SliceParser and StreamParser
//!
//! This module consolidates surrogate pair testing logic to ensure both parsers
//! handle UTF-16 surrogate pairs identically across different configurations.

use picojson::{
    ChunkReader, DefaultConfig, Event, PullParser, SliceParser, StreamParser, String as JsonString,
};

/// Test fixture that runs a JSON input against any PullParser implementation
/// and verifies the expected sequence of events.
fn test_fixture<P: PullParser>(mut parser: P, expected_events: &[Event]) {
    for (i, expected) in expected_events.iter().enumerate() {
        let actual = parser
            .next_event()
            .unwrap_or_else(|e| panic!("Parser error at event {}: {:?}", i, e));

        match (expected, &actual) {
            // Handle string content comparison
            (Event::String(expected_str), Event::String(actual_str)) => {
                assert_eq!(
                    expected_str.as_ref(),
                    actual_str.as_ref(),
                    "String content mismatch at event {}",
                    i
                );
            }
            (Event::Key(expected_key), Event::Key(actual_key)) => {
                assert_eq!(
                    expected_key.as_ref(),
                    actual_key.as_ref(),
                    "Key content mismatch at event {}",
                    i
                );
            }
            _ => {
                assert_eq!(
                    std::mem::discriminant(expected),
                    std::mem::discriminant(&actual),
                    "Event type mismatch at event {}: expected {:?}, got {:?}",
                    i,
                    expected,
                    actual
                );
            }
        }
    }
}

/// Test fixture for cases that should error during parsing
fn test_error_fixture<P: PullParser>(mut parser: P, error_description: &str) {
    // Parse until we find the first event that should cause an error
    loop {
        match parser.next_event() {
            Ok(Event::EndDocument) => {
                panic!(
                    "{}: Expected error but parsing completed successfully",
                    error_description
                );
            }
            Ok(_) => continue, // Keep parsing until we hit the error
            Err(_) => return,  // Got expected error
        }
    }
}

/// Create a SliceParser for testing
fn create_slice_parser<'a>(input: &'a str, scratch_buffer: &'a mut [u8]) -> SliceParser<'a, 'a> {
    scratch_buffer.fill(0); // Clear the buffer
    SliceParser::with_buffer(input, scratch_buffer)
}

/// Create a StreamParser with full-size chunks for testing
fn create_stream_parser_full<'a>(
    input: &'a str,
    buffer: &'a mut [u8],
) -> StreamParser<'a, ChunkReader<'a>, DefaultConfig> {
    buffer.fill(0); // Clear the buffer
    let reader = ChunkReader::new(input.as_bytes(), input.len());
    StreamParser::with_config(reader, buffer)
}

/// Create a StreamParser with small chunks to test buffer boundaries
fn create_stream_parser_chunked<'a>(
    input: &'a str,
    chunk_size: usize,
    buffer: &'a mut [u8],
) -> StreamParser<'a, ChunkReader<'a>, DefaultConfig> {
    buffer.fill(0); // Clear the buffer
    let reader = ChunkReader::new(input.as_bytes(), chunk_size);
    StreamParser::with_config(reader, buffer)
}

#[test]
fn test_basic_surrogate_pair() {
    let input = r#"["\uD801\uDC37"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("𐐷")),
        Event::EndArray,
        Event::EndDocument,
    ];

    // Test all parser configurations
    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 8, &mut buffer),
        &expected,
    );
    test_fixture(
        create_stream_parser_chunked(input, 3, &mut buffer),
        &expected,
    ); // Very small chunks
}

#[test]
fn test_musical_clef_surrogate_pair() {
    let input = r#"["\uD834\uDD1E"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("𝄞")),
        Event::EndArray,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 6, &mut buffer),
        &expected,
    );
}

#[test]
fn test_multiple_surrogate_pairs() {
    let input = r#"["\uD801\uDC37\uD834\uDD1E"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("𐐷𝄞")),
        Event::EndArray,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 10, &mut buffer),
        &expected,
    );
}

#[test]
fn test_surrogate_pairs_in_object_keys() {
    let input = r#"{"\uD801\uDC37": "value"}"#;
    let expected = [
        Event::StartObject,
        Event::Key(JsonString::Unescaped("𐐷")),
        Event::String(JsonString::Borrowed("value")),
        Event::EndObject,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 7, &mut buffer),
        &expected,
    );
}

#[test]
fn test_mixed_content_with_surrogate_pairs() {
    let input = r#"{"text": "Hello \uD801\uDC37 World"}"#;
    let expected = [
        Event::StartObject,
        Event::Key(JsonString::Borrowed("text")),
        Event::String(JsonString::Unescaped("Hello 𐐷 World")),
        Event::EndObject,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 5, &mut buffer),
        &expected,
    );
}

#[test]
fn test_edge_of_surrogate_ranges() {
    // D7FF is just below high surrogate range, E000 is just above low surrogate range
    let input = r#"["\uD7FF\uE000"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("\u{D7FF}\u{E000}")), // Two separate characters: \uD7FF + \uE000
        Event::EndArray,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 4, &mut buffer),
        &expected,
    );
}

// Error cases

#[test]
fn test_lone_low_surrogate_error() {
    let input = r#"["\uDC37"]"#;

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_error_fixture(
        create_slice_parser(input, &mut scratch),
        "Lone low surrogate",
    );
    test_error_fixture(
        create_stream_parser_full(input, &mut buffer),
        "Lone low surrogate",
    );
    test_error_fixture(
        create_stream_parser_chunked(input, 5, &mut buffer),
        "Lone low surrogate",
    );
}

#[test]
fn test_high_surrogate_followed_by_non_low_surrogate_error() {
    let input = r#"["\uD801\u0041"]"#;

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_error_fixture(
        create_slice_parser(input, &mut scratch),
        "High surrogate + non-low surrogate",
    );
    test_error_fixture(
        create_stream_parser_full(input, &mut buffer),
        "High surrogate + non-low surrogate",
    );
    test_error_fixture(
        create_stream_parser_chunked(input, 6, &mut buffer),
        "High surrogate + non-low surrogate",
    );
}

#[test]
fn test_double_high_surrogate_error() {
    let input = r#"["\uD801\uD802"]"#;

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_error_fixture(
        create_slice_parser(input, &mut scratch),
        "High surrogate + high surrogate",
    );
    test_error_fixture(
        create_stream_parser_full(input, &mut buffer),
        "High surrogate + high surrogate",
    );
    test_error_fixture(
        create_stream_parser_chunked(input, 8, &mut buffer),
        "High surrogate + high surrogate",
    );
}

#[test]
fn test_interrupted_surrogate_pair_error() {
    // This is the critical test case that was previously failing
    // \n should clear the pending high surrogate, making \uDC37 a lone low surrogate
    let input = r#"["\uD801\n\uDC37"]"#;

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_error_fixture(
        create_slice_parser(input, &mut scratch),
        "Interrupted surrogate pair",
    );
    test_error_fixture(
        create_stream_parser_full(input, &mut buffer),
        "Interrupted surrogate pair",
    );
    test_error_fixture(
        create_stream_parser_chunked(input, 4, &mut buffer),
        "Interrupted surrogate pair",
    );
}

#[test]
fn test_various_escape_interruptions() {
    // Test different types of simple escapes that should clear surrogate state
    let test_cases = [
        (r#"["\uD801\t\uDC37"]"#, "Tab escape interruption"),
        (r#"["\uD801\r\uDC37"]"#, "Carriage return interruption"),
        (r#"["\uD801\\\uDC37"]"#, "Backslash escape interruption"),
        (r#"["\uD801\"\uDC37"]"#, "Quote escape interruption"),
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    for (input, description) in &test_cases {
        test_error_fixture(create_slice_parser(input, &mut scratch), description);
        test_error_fixture(create_stream_parser_full(input, &mut buffer), description);
        test_error_fixture(
            create_stream_parser_chunked(input, 3, &mut buffer),
            description,
        );
    }
}

// Buffer boundary specific tests for StreamParser

#[test]
fn test_surrogate_pair_across_chunk_boundaries() {
    let input = r#"["\uD801\uDC37"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("𐐷")),
        Event::EndArray,
        Event::EndDocument,
    ];

    let mut buffer = [0u8; 1024];

    // Test with chunk boundaries that split the surrogate pair
    test_fixture(
        create_stream_parser_chunked(input, 1, &mut buffer),
        &expected,
    ); // Every byte
    test_fixture(
        create_stream_parser_chunked(input, 7, &mut buffer),
        &expected,
    ); // Split between surrogates
    test_fixture(
        create_stream_parser_chunked(input, 11, &mut buffer),
        &expected,
    ); // Split within low surrogate
}

#[test]
fn test_very_small_buffers() {
    let input = r#"["\uD801\uDC37"]"#;
    let expected = [
        Event::StartArray,
        Event::String(JsonString::Unescaped("𐐷")),
        Event::EndArray,
        Event::EndDocument,
    ];

    let mut buffer = [0u8; 1024];

    // Test with extremely small chunks to stress buffer management
    test_fixture(
        create_stream_parser_chunked(input, 1, &mut buffer),
        &expected,
    );
    test_fixture(
        create_stream_parser_chunked(input, 2, &mut buffer),
        &expected,
    );
}

#[test]
fn test_pathological_cases() {
    // High surrogate at end of string (should error)
    let input1 = r#"["\uD801"]"#;

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_error_fixture(
        create_slice_parser(input1, &mut scratch),
        "High surrogate at end of string",
    );
    test_error_fixture(
        create_stream_parser_full(input1, &mut buffer),
        "High surrogate at end of string",
    );
    test_error_fixture(
        create_stream_parser_chunked(input1, 3, &mut buffer),
        "High surrogate at end of string",
    );
}

#[test]
fn test_complex_nested_structures() {
    let input = r#"{"users": [{"name": "\uD801\uDC37", "emoji": "\uD834\uDD1E"}]}"#;
    let expected = [
        Event::StartObject,
        Event::Key(JsonString::Borrowed("users")),
        Event::StartArray,
        Event::StartObject,
        Event::Key(JsonString::Borrowed("name")),
        Event::String(JsonString::Unescaped("𐐷")),
        Event::Key(JsonString::Borrowed("emoji")),
        Event::String(JsonString::Unescaped("𝄞")),
        Event::EndObject,
        Event::EndArray,
        Event::EndObject,
        Event::EndDocument,
    ];

    let mut scratch = [0u8; 1024];
    let mut buffer = [0u8; 1024];

    test_fixture(create_slice_parser(input, &mut scratch), &expected);
    test_fixture(create_stream_parser_full(input, &mut buffer), &expected);
    test_fixture(
        create_stream_parser_chunked(input, 12, &mut buffer),
        &expected,
    );
}
