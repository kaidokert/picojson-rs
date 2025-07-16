// SPDX-License-Identifier: Apache-2.0

use crate::ParseError;
/// Shared components for JSON parsers
use crate::{JsonNumber, String};

/// Events produced by JSON parsers
#[derive(Debug, PartialEq)]
pub enum Event<'a, 'b> {
    /// The start of an object (e.g., `{`).
    StartObject,
    /// The end of an object (e.g., `}`).
    EndObject,
    /// The start of an array (e.g., `[`).
    StartArray,
    /// The end of an array (e.g., `]`).
    EndArray,
    /// An object key (e.g., `"key":`).
    Key(String<'a, 'b>),
    /// A string value (e.g., `"value"`).
    String(String<'a, 'b>),
    /// A number value (e.g., `42` or `3.14`).
    Number(JsonNumber<'a, 'b>),
    /// A boolean value (e.g., `true` or `false`).
    Bool(bool),
    /// A null value (e.g., `null`).
    Null,
    /// End of the document.
    EndDocument,
}

/// Specific unexpected states that can occur during parsing.
#[derive(Debug, PartialEq)]
pub enum UnexpectedState {
    /// A generic state mismatch occurred.
    StateMismatch,
    /// An invalid escape token was encountered.
    InvalidEscapeToken,
    /// A Unicode escape sequence was invalid.
    InvalidUnicodeEscape,
    /// An operation exceeded the buffer's capacity.
    BufferCapacityExceeded,
    /// Invalid slice bounds were provided for an operation.
    InvalidSliceBounds,
}

/// Internal parser state tracking
#[derive(Debug, PartialEq)]
pub enum State {
    None,
    Key(usize),
    String(usize),
    Number(usize),
}

/// Parser state and event storage
pub struct ParserState {
    pub evts: [Option<crate::ujson::Event>; 2],
}

impl ParserState {
    pub fn new() -> Self {
        Self {
            evts: core::array::from_fn(|_| None),
        }
    }
}

impl Default for ParserState {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for parsers that can be used in a pull-based manner.
///
/// This trait is implemented by both `SliceParser` and `StreamParser`.
pub trait PullParser {
    /// Iterator-like method that returns None when parsing is complete.
    /// This method returns None when EndDocument is reached, Some(Ok(event)) for successful events,
    /// and Some(Err(error)) for parsing errors.
    fn next(&mut self) -> Option<Result<Event<'_, '_>, ParseError>> {
        match self.next_event() {
            Ok(Event::EndDocument) => None,
            other => Some(other),
        }
    }
    /// Returns the next JSON event or an error if parsing fails.
    /// Parsing continues until `EndDocument` is returned or an error occurs.
    fn next_event(&mut self) -> Result<Event<'_, '_>, ParseError>;
}

/// Utility for calculating common content range boundaries in JSON parsing.
/// Provides consistent position arithmetic for string/number content extraction.
pub(crate) struct ContentRange;

impl ContentRange {
    /// Calculate number content start from current position
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position (typically after first digit was processed)
    ///
    /// # Returns
    /// Position that includes the first digit of the number
    pub fn number_start_from_current(current_pos: usize) -> usize {
        current_pos.saturating_sub(1) // Back up to include first digit
    }

    /// Calculate string content boundaries using content start position
    /// Alternative to string_content_bounds that works with content positions
    ///
    /// # Arguments
    /// * `content_start` - Position where content begins (after opening quote)
    /// * `current_pos` - Current parser position (typically after closing quote)
    ///
    /// # Returns
    /// (content_start, content_end) where both positions bound the actual content
    pub fn string_content_bounds_from_content_start(
        content_start: usize,
        current_pos: usize,
    ) -> (usize, usize) {
        let content_end = current_pos.saturating_sub(1); // Back up to exclude closing quote
        (content_start, content_end)
    }

    /// Calculate Unicode escape sequence boundaries
    ///
    /// # Arguments
    /// * `current_pos` - Current position (after 4 hex digits)
    ///
    /// # Returns
    /// (hex_start, hex_end, escape_start) where hex_start/hex_end bound the XXXX
    /// and escape_start is the position of the backslash in \uXXXX
    pub fn unicode_escape_bounds(current_pos: usize) -> (usize, usize, usize) {
        let hex_start = current_pos.saturating_sub(4); // Start of XXXX
        let hex_end = current_pos; // End of XXXX
        let escape_start = current_pos.saturating_sub(6); // Start of \uXXXX
        (hex_start, hex_end, escape_start)
    }

    /// Calculate end position for string content
    /// Used when the parser position needs to exclude the delimiter
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position
    ///
    /// # Returns
    /// Position excluding the final delimiter
    pub fn end_position_excluding_delimiter(current_pos: usize) -> usize {
        current_pos.saturating_sub(1)
    }

    /// Calculate number end position with delimiter handling
    /// Standardizes the pattern of excluding delimiters unless at document end
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position
    /// * `use_full_span` - True if the number is at the end of the document and not in a container,
    ///   meaning there is no delimiter to exclude.
    ///
    /// # Returns
    /// End position for number content
    pub fn number_end_position(current_pos: usize, use_full_span: bool) -> usize {
        if use_full_span {
            // At document end and standalone - use full span (no delimiter to exclude)
            current_pos
        } else {
            // Normal case - exclude delimiter
            current_pos.saturating_sub(1)
        }
    }
}

pub fn from_utf8(v: &[u8]) -> Result<&str, ParseError> {
    core::str::from_utf8(v).map_err(Into::into)
}
