// SPDX-License-Identifier: Apache-2.0

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

/// Errors that can occur during JSON parsing
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// An error bubbled up from the underlying tokenizer.
    TokenizerError,
    /// The provided scratch buffer was not large enough for an operation.
    ScratchBufferFull,
    /// A string slice was not valid UTF-8.
    InvalidUtf8(core::str::Utf8Error),
    /// A number string could not be parsed.
    InvalidNumber,
    /// The parser entered an unexpected internal state.
    UnexpectedState(&'static str),
    /// End of input data.
    EndOfData,
    /// Invalid hex digits in Unicode escape sequence.
    InvalidUnicodeHex,
    /// Valid hex but invalid Unicode codepoint.
    InvalidUnicodeCodepoint,
    /// Invalid escape sequence character.
    InvalidEscapeSequence,
    /// Float encountered but float support is disabled and float-error is configured
    FloatNotAllowed,
    /// A JSON token was too large to fit in the available buffer space
    TokenTooLarge {
        token_size: usize,
        buffer_size: usize,
        suggestion: &'static str,
    },
    /// End of input stream was reached unexpectedly
    EndOfStream,
    /// Error from the underlying reader (I/O error, not end-of-stream)
    ReaderError,
    /// Numeric overflow
    NumericOverflow,
}

impl From<core::str::Utf8Error> for ParseError {
    fn from(err: core::str::Utf8Error) -> Self {
        ParseError::InvalidUtf8(err)
    }
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
pub(super) struct ParserState {
    pub state: State,
    pub evts: [Option<crate::ujson::Event>; 2],
}

impl ParserState {
    pub fn new() -> Self {
        Self {
            state: State::None,
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
}

/// Utility for common error handling patterns in JSON parsing.
/// Provides consistent error creation and UTF-8 validation across parsers.
pub(crate) struct ParserErrorHandler;

pub const fn from_utf8(v: &[u8]) -> Result<&str, ParseError> {
    match core::str::from_utf8(v) {
        Ok(s) => Ok(s),
        Err(e) => Err(ParseError::InvalidUtf8(e)),
    }
}

impl ParserErrorHandler {
    /// Create an UnexpectedState error with context
    ///
    /// # Arguments
    /// * `context` - Description of what state was unexpected
    ///
    /// # Returns
    /// ParseError::UnexpectedState with the given context
    pub fn unexpected_state(context: &'static str) -> ParseError {
        ParseError::UnexpectedState(context)
    }

    /// Create a state mismatch error for parser state validation
    ///
    /// # Arguments
    /// * `expected` - The expected parser state
    /// * `operation` - The operation that failed
    ///
    /// # Returns
    /// ParseError::UnexpectedState with formatted message
    pub fn state_mismatch(expected: &'static str, operation: &'static str) -> ParseError {
        // Since we can't use format! in no_std, we'll use predefined common patterns
        match (expected, operation) {
            ("string", "end") => ParseError::UnexpectedState("String end without String start"),
            ("key", "end") => ParseError::UnexpectedState("Key end without Key start"),
            ("number", "extract") => ParseError::UnexpectedState("Not in number state"),
            ("active", "process") => ParseError::UnexpectedState("Not in active processing state"),
            _ => ParseError::UnexpectedState("State mismatch"),
        }
    }

    /// Create error for invalid Unicode escape length
    pub fn invalid_unicode_length() -> ParseError {
        ParseError::UnexpectedState("Invalid Unicode escape length")
    }

    /// Create error for incomplete Unicode escape sequences
    pub fn incomplete_unicode_escape() -> ParseError {
        ParseError::UnexpectedState("Incomplete Unicode escape sequence")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unicode_escape_bounds() {
        // Test unicode_escape_bounds with typical position after \u1234
        let current_pos = 10; // Position after reading \u1234
        let (hex_start, hex_end, escape_start) = ContentRange::unicode_escape_bounds(current_pos);

        assert_eq!(hex_start, 6); // Start of XXXX (10 - 4)
        assert_eq!(hex_end, 10); // End of XXXX
        assert_eq!(escape_start, 4); // Start of \uXXXX (10 - 6)
    }

    #[test]
    fn test_unicode_escape_bounds_edge_cases() {
        // Test with position that would underflow
        let (hex_start, hex_end, escape_start) = ContentRange::unicode_escape_bounds(3);
        assert_eq!(hex_start, 0); // saturating_sub prevents underflow
        assert_eq!(hex_end, 3);
        assert_eq!(escape_start, 0); // saturating_sub prevents underflow
    }

    #[test]
    fn test_error_constructors() {
        // Test state_mismatch error constructor
        let error = ParserErrorHandler::state_mismatch("string", "end");
        match error {
            ParseError::UnexpectedState(msg) => {
                assert_eq!(msg, "String end without String start");
            }
            _ => panic!("Expected UnexpectedState error"),
        }

        // Test invalid_unicode_length error constructor
        let error = ParserErrorHandler::invalid_unicode_length();
        match error {
            ParseError::UnexpectedState(msg) => {
                assert_eq!(msg, "Invalid Unicode escape length");
            }
            _ => panic!("Expected UnexpectedState error"),
        }

        // Test incomplete_unicode_escape error constructor
        let error = ParserErrorHandler::incomplete_unicode_escape();
        match error {
            ParseError::UnexpectedState(msg) => {
                assert_eq!(msg, "Incomplete Unicode escape sequence");
            }
            _ => panic!("Expected UnexpectedState error"),
        }
    }

    #[test]
    fn test_utf8_error_conversion() {
        // Test From<Utf8Error> trait implementation
        use core::str;
        // Create a proper invalid UTF-8 sequence (lone continuation byte) dynamically
        // to avoid compile-time warning about static invalid UTF-8 literals
        let mut invalid_utf8_array = [0u8; 1];
        invalid_utf8_array[0] = 0b10000000u8; // Invalid UTF-8 - continuation byte without start
        let invalid_utf8 = &invalid_utf8_array;

        match str::from_utf8(invalid_utf8) {
            Err(utf8_error) => {
                let parse_error: ParseError = utf8_error.into();
                match parse_error {
                    ParseError::InvalidUtf8(_) => {
                        // Expected - conversion works correctly
                    }
                    _ => panic!("Expected InvalidUtf8 error"),
                }
            }
            Ok(_) => panic!("Expected UTF-8 validation to fail"),
        }
    }

    #[test]
    fn test_string_content_bounds_from_content_start() {
        // Test string content bounds using content start position
        let content_start = 6; // After opening quote
        let current_pos = 15; // After closing quote

        let (start, end) =
            ContentRange::string_content_bounds_from_content_start(content_start, current_pos);
        assert_eq!(start, 6); // Same as input content_start
        assert_eq!(end, 14); // current_pos - 1 (before closing quote)
    }

    #[test]
    fn test_string_content_bounds_from_content_start_edge_cases() {
        // Test with minimum positions
        let (start, end) = ContentRange::string_content_bounds_from_content_start(0, 1);
        assert_eq!(start, 0);
        assert_eq!(end, 0); // 1 - 1

        // Test with underflow protection
        let (start, end) = ContentRange::string_content_bounds_from_content_start(5, 0);
        assert_eq!(start, 5);
        assert_eq!(end, 0); // saturating_sub protects from underflow
    }
}
