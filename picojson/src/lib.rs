// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

// Compile-time configuration validation
mod config_check;

mod tokenizer;
// Temporary internal alias, not exported
use tokenizer as ujson;
pub use tokenizer::ArrayBitStack;
pub use tokenizer::BitStack;

// NEW API exports
pub use tokenizer::ArrayBitBucket;
pub use tokenizer::{BitBucket, BitStackConfig, BitStackStruct, DefaultConfig, DepthCounter};

mod copy_on_escape;

mod escape_processor;

mod direct_buffer;

mod direct_parser;

mod flex_parser;

mod shared;
pub use shared::{Event, ParseError};
pub use ujson::BitStackCore;

mod slice_input_buffer;

mod json_number;
use json_number::parse_number_from_str;
pub use json_number::{JsonNumber, NumberResult};

mod json_string;
pub use json_string::String;

mod number_parser;

pub use direct_parser::{DirectParser, Reader};
pub use flex_parser::PullParser;

impl From<slice_input_buffer::Error> for ParseError {
    fn from(err: slice_input_buffer::Error) -> Self {
        match err {
            slice_input_buffer::Error::ReachedEnd => ParseError::EndOfData,
            slice_input_buffer::Error::InvalidSliceBounds => {
                ParseError::UnexpectedState("Invalid slice bounds in input buffer")
            }
        }
    }
}
