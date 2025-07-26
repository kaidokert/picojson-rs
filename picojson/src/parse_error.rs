// SPDX-License-Identifier: Apache-2.0

use crate::shared::UnexpectedState;
use crate::slice_input_buffer;
use crate::stream_buffer;

use crate::ujson;

/// Errors that can occur during JSON parsing
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// An error bubbled up from the underlying tokenizer.
    TokenizerError(ujson::Error),
    /// A UTF-8 error occurred.
    Utf8(core::str::Utf8Error),
    /// The scratch buffer is full.
    ScratchBufferFull,
    /// The input buffer is full.
    InputBufferFull,
    /// A number string could not be parsed.
    InvalidNumber,
    /// The parser entered an unexpected internal state.
    Unexpected(UnexpectedState),
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
    /// Error from the underlying reader (I/O error, not end-of-stream)
    ReaderError,
    /// Numeric overflow
    NumericOverflow,
}

impl From<slice_input_buffer::Error> for ParseError {
    fn from(err: slice_input_buffer::Error) -> Self {
        match err {
            slice_input_buffer::Error::ReachedEnd => ParseError::EndOfData,
            slice_input_buffer::Error::InvalidSliceBounds => {
                UnexpectedState::InvalidSliceBounds.into()
            }
        }
    }
}

impl From<stream_buffer::StreamBufferError> for ParseError {
    fn from(err: stream_buffer::StreamBufferError) -> Self {
        match err {
            stream_buffer::StreamBufferError::BufferFull => ParseError::ScratchBufferFull,
            stream_buffer::StreamBufferError::EndOfData => ParseError::EndOfData,
            stream_buffer::StreamBufferError::Unexpected => {
                ParseError::Unexpected(UnexpectedState::BufferCapacityExceeded)
            }
            stream_buffer::StreamBufferError::InvalidSliceBounds => {
                ParseError::Unexpected(UnexpectedState::InvalidSliceBounds)
            }
        }
    }
}

impl From<core::str::Utf8Error> for ParseError {
    fn from(err: core::str::Utf8Error) -> Self {
        ParseError::Utf8(err)
    }
}

impl From<UnexpectedState> for ParseError {
    fn from(info: UnexpectedState) -> Self {
        ParseError::Unexpected(info)
    }
}

impl From<ujson::Error> for ParseError {
    fn from(err: ujson::Error) -> Self {
        ParseError::TokenizerError(err)
    }
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseError::TokenizerError(e) => write!(f, "{e}"),
            ParseError::Utf8(e) => write!(f, "Invalid UTF-8: {e}"),
            _ => write!(f, "{self:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_constructors() {
        // Test state_mismatch error constructor
        let error: ParseError = UnexpectedState::StateMismatch.into();
        match error {
            ParseError::Unexpected(info) => {
                assert_eq!(info, UnexpectedState::StateMismatch);
            }
            _ => panic!("Expected UnexpectedState error"),
        }

        // Test invalid_unicode_length error constructor
        let error: ParseError = UnexpectedState::InvalidUnicodeEscape.into();
        match error {
            ParseError::Unexpected(info) => {
                assert_eq!(info, UnexpectedState::InvalidUnicodeEscape);
            }
            _ => panic!("Expected UnexpectedState error"),
        }

        // Test incomplete_unicode_escape error constructor
        let error: ParseError = UnexpectedState::InvalidUnicodeEscape.into();
        match error {
            ParseError::Unexpected(info) => {
                assert_eq!(info, UnexpectedState::InvalidUnicodeEscape);
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
                    ParseError::Utf8(_) => {
                        // Expected - conversion works correctly
                    }
                    _ => panic!("Expected Utf8 error"),
                }
            }
            Ok(_) => panic!("Expected UTF-8 validation to fail"),
        }
    }
}
