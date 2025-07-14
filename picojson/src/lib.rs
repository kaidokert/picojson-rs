//! A minimal JSON pull-parser for resource-constrained environments.
//!
//! `picojson` provides low-level, `no_std` compatible pull-parsers that operate without
//! recursion or heap allocations, designed for embedded systems and memory-limited scenarios.
//!
//! ## Main Types
//!
//! - [`SliceParser`] - Parses JSON from byte slices or strings with zero-copy when possible
//! - [`StreamParser`] - Parses JSON from any [`Reader`] source, buffering as needed
//!
//! Both parsers emit [`Event`]s representing JSON structure and values, allowing fine-grained
//! control over parsing and memory usage.
//!
//! ## Quick Start
//!
//! ```rust
//! use picojson::{SliceParser, Event, String, PullParser};
//!
//! let json = r#"{"name": "value"}"#;
//! let mut parser = SliceParser::new(json);
//!
//! while let Some(event) = parser.next() {
//!     match event.expect("Parse error") {
//!         Event::Key(key) => println!("Found key: {}", key),
//!         _ => {}
//!     }
//! }
//! ```
//!
//! ## String Escapes
//!
//! For JSON containing escape sequences (like `\n`, `\"`, `\u0041`), use constructors
//! with scratch buffers to handle unescaping. The buffer must be at least as long
//! as the longest contiguous string or number in your JSON:
//!
//! ```rust
//! # use picojson::SliceParser;
//! let json = r#"{"msg": "Hello\nWorld"}"#;
//! let mut scratch = [0u8; 32];
//! let parser = SliceParser::with_buffer(json, &mut scratch);
//! ```
//!
//! ## More Examples
//!
//! For advanced usage including configurable nesting depth, number parsing options,
//! and stream parsing, see the [examples directory](https://github.com/kaidokert/picojson-rs/tree/main/picojson/examples)
//! on GitHub.

// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

// Compile-time configuration validation
mod config_check;

mod ujson;
pub use ujson::ArrayBitStack;

pub use ujson::ArrayBitBucket;
pub use ujson::{BitBucket, BitStackConfig, BitStackStruct, DefaultConfig, DepthCounter};

mod copy_on_escape;

mod escape_processor;

mod content_builder;

mod parser_core;

mod stream_buffer;

mod stream_content_builder;

mod stream_parser;

#[cfg(test)]
mod debug_test;

#[cfg(test)]
mod simple_debug;

mod slice_content_builder;

mod slice_parser;

mod parse_error;
pub use parse_error::ParseError;

mod shared;
pub use shared::{Event, PullParser};

mod event_processor;

mod slice_input_buffer;

mod json_number;
use json_number::parse_number_from_str;
pub use json_number::{JsonNumber, NumberResult};

mod json_string;
pub use json_string::String;

mod int_parser;
mod number_parser;

pub use slice_parser::SliceParser;
pub use stream_parser::{Reader, StreamParser};

mod chunk_reader;
pub use chunk_reader::ChunkReader;
