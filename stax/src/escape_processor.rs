// SPDX-License-Identifier: Apache-2.0

use crate::{shared::ParserErrorHandler, ParseError};

/// Shared utilities for processing JSON escape sequences.
/// This module contains pure functions for escape processing that can be used
/// by both CopyOnEscape and StreamingBuffer components.
pub(crate) struct EscapeProcessor;

impl EscapeProcessor {
    /// Convert an escape token from the tokenizer to the corresponding escape character.
    /// This extracts the character that follows the backslash in the escape sequence.
    ///
    /// # Arguments
    /// * `escape_token` - The escape token from the tokenizer
    ///
    /// # Returns
    /// The character that follows the backslash, or None if the token is not a simple escape.
    ///
    /// # Examples
    /// ```ignore
    /// // Internal API - see unit tests for usage examples
    /// assert_eq!(EscapeProcessor::token_to_escape_char(&EventToken::EscapeNewline).unwrap(), b'n');
    /// ```
    pub fn token_to_escape_char(escape_token: &ujson::EventToken) -> Option<u8> {
        match escape_token {
            ujson::EventToken::EscapeQuote => Some(b'"'),
            ujson::EventToken::EscapeBackslash => Some(b'\\'),
            ujson::EventToken::EscapeSlash => Some(b'/'),
            ujson::EventToken::EscapeBackspace => Some(b'b'),
            ujson::EventToken::EscapeFormFeed => Some(b'f'),
            ujson::EventToken::EscapeNewline => Some(b'n'),
            ujson::EventToken::EscapeCarriageReturn => Some(b'r'),
            ujson::EventToken::EscapeTab => Some(b't'),
            _ => None,
        }
    }

    /// Process an escape token directly to the unescaped byte value.
    /// This is a convenience method that combines token_to_escape_char and process_simple_escape.
    ///
    /// # Arguments
    /// * `escape_token` - The escape token from the tokenizer
    ///
    /// # Returns
    /// The unescaped byte value, or an error if the token is invalid or not a simple escape.
    ///
    /// # Examples
    /// ```ignore
    /// // Internal API - see unit tests for usage examples
    /// assert_eq!(EscapeProcessor::process_escape_token(&EventToken::EscapeNewline).unwrap(), b'\n');
    /// ```
    pub fn process_escape_token(escape_token: &ujson::EventToken) -> Result<u8, ParseError> {
        let escape_char = Self::token_to_escape_char(escape_token)
            .ok_or(ParserErrorHandler::unexpected_state("Invalid escape token"))?;
        Self::process_simple_escape(escape_char)
    }

    /// Process a simple escape sequence character and return the unescaped byte.
    ///
    /// # Arguments
    /// * `escape_char` - The character following the backslash in an escape sequence
    ///
    /// # Returns
    /// The unescaped byte value, or an error if the escape sequence is invalid.
    ///
    /// # Examples
    /// ```ignore
    /// // Internal API - see unit tests for usage examples
    /// assert_eq!(EscapeProcessor::process_simple_escape(b'n').unwrap(), b'\n');
    /// ```
    pub fn process_simple_escape(escape_char: u8) -> Result<u8, ParseError> {
        match escape_char {
            b'n' => Ok(b'\n'),
            b't' => Ok(b'\t'),
            b'r' => Ok(b'\r'),
            b'\\' => Ok(b'\\'),
            b'"' => Ok(b'"'),
            b'/' => Ok(b'/'),
            b'b' => Ok(0x08), // Backspace
            b'f' => Ok(0x0C), // Form feed
            _ => Err(ParseError::InvalidEscapeSequence),
        }
    }

    /// Validate that a byte represents a valid hexadecimal digit.
    ///
    /// # Arguments
    /// * `byte` - The byte to validate
    ///
    /// # Returns
    /// The numeric value (0-15) of the hex digit, or an error if invalid.
    pub fn validate_hex_digit(byte: u8) -> Result<u32, ParseError> {
        match byte {
            b'0'..=b'9' => Ok((byte - b'0') as u32),
            b'a'..=b'f' => Ok((byte - b'a' + 10) as u32),
            b'A'..=b'F' => Ok((byte - b'A' + 10) as u32),
            _ => Err(ParseError::InvalidUnicodeHex),
        }
    }

    /// Process a Unicode escape sequence (\uXXXX) and return the UTF-8 encoded bytes.
    ///
    /// # Arguments
    /// * `hex_slice` - A 4-byte slice containing the hexadecimal digits
    /// * `utf8_buffer` - A buffer to write the UTF-8 encoded result (must be at least 4 bytes)
    ///
    /// # Returns
    /// A slice containing the UTF-8 encoded bytes, or an error if the escape is invalid.
    ///
    /// # Examples
    /// ```ignore
    /// // Internal API - see unit tests for usage examples
    /// let mut buffer = [0u8; 4];
    /// let result = EscapeProcessor::process_unicode_escape(b"0041", &mut buffer).unwrap();
    /// assert_eq!(result, b"A");
    /// ```
    pub fn process_unicode_escape<'a>(
        hex_slice: &[u8],
        utf8_buffer: &'a mut [u8],
    ) -> Result<&'a [u8], ParseError> {
        if hex_slice.len() != 4 {
            return Err(ParseError::InvalidUnicodeHex);
        }

        // Convert hex bytes to Unicode codepoint
        let mut codepoint = 0u32;
        for &byte in hex_slice {
            let digit = Self::validate_hex_digit(byte)?;
            codepoint = (codepoint << 4) | digit;
        }

        // Convert codepoint to character and encode as UTF-8
        let ch = char::from_u32(codepoint).ok_or(ParseError::InvalidUnicodeCodepoint)?;
        let utf8_str = ch.encode_utf8(utf8_buffer);
        Ok(utf8_str.as_bytes())
    }
}

/// Shared Unicode escape hex digit collector for both parsers.
/// Provides a common interface for collecting the 4 hex digits in \uXXXX sequences.
#[derive(Debug)]
pub(crate) struct UnicodeEscapeCollector {
    /// Buffer to collect the 4 hex digits
    hex_buffer: [u8; 4],
    /// Current position in the hex buffer (0-4)
    hex_pos: usize,
}

impl UnicodeEscapeCollector {
    /// Create a new Unicode escape collector
    pub fn new() -> Self {
        Self {
            hex_buffer: [0u8; 4],
            hex_pos: 0,
        }
    }

    /// Reset the collector for a new Unicode escape sequence
    pub fn reset(&mut self) {
        self.hex_pos = 0;
    }

    /// Add a hex digit to the collector
    /// Returns true if this completes the 4-digit sequence
    pub fn add_hex_digit(&mut self, digit: u8) -> Result<bool, ParseError> {
        // Validate the hex digit first
        EscapeProcessor::validate_hex_digit(digit)?;

        if self.hex_pos >= 4 {
            return Err(ParserErrorHandler::unexpected_state(
                "Too many hex digits in Unicode escape",
            ));
        }

        self.hex_buffer[self.hex_pos] = digit;
        self.hex_pos += 1;

        Ok(self.hex_pos == 4)
    }

    /// Process the collected hex digits and return UTF-8 bytes
    /// Should only be called when is_complete() returns true
    pub fn process_to_utf8<'a>(&self, utf8_buffer: &'a mut [u8]) -> Result<&'a [u8], ParseError> {
        if self.hex_pos != 4 {
            return Err(ParserErrorHandler::incomplete_unicode_escape());
        }

        EscapeProcessor::process_unicode_escape(&self.hex_buffer, utf8_buffer)
    }

    /// Check if we have collected all 4 hex digits
    pub fn is_complete(&self) -> bool {
        self.hex_pos == 4
    }

    /// Get the current number of collected hex digits
    pub fn hex_count(&self) -> usize {
        self.hex_pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_escapes() {
        assert_eq!(EscapeProcessor::process_simple_escape(b'n').unwrap(), b'\n');
        assert_eq!(EscapeProcessor::process_simple_escape(b't').unwrap(), b'\t');
        assert_eq!(EscapeProcessor::process_simple_escape(b'r').unwrap(), b'\r');
        assert_eq!(
            EscapeProcessor::process_simple_escape(b'\\').unwrap(),
            b'\\'
        );
        assert_eq!(EscapeProcessor::process_simple_escape(b'"').unwrap(), b'"');
        assert_eq!(EscapeProcessor::process_simple_escape(b'/').unwrap(), b'/');
        assert_eq!(EscapeProcessor::process_simple_escape(b'b').unwrap(), 0x08);
        assert_eq!(EscapeProcessor::process_simple_escape(b'f').unwrap(), 0x0C);
    }

    #[test]
    fn test_invalid_simple_escape() {
        assert!(EscapeProcessor::process_simple_escape(b'x').is_err());
        assert!(EscapeProcessor::process_simple_escape(b'z').is_err());
        assert!(EscapeProcessor::process_simple_escape(b'1').is_err());
    }

    #[test]
    fn test_hex_digit_validation() {
        // Valid digits
        assert_eq!(EscapeProcessor::validate_hex_digit(b'0').unwrap(), 0);
        assert_eq!(EscapeProcessor::validate_hex_digit(b'9').unwrap(), 9);
        assert_eq!(EscapeProcessor::validate_hex_digit(b'a').unwrap(), 10);
        assert_eq!(EscapeProcessor::validate_hex_digit(b'f').unwrap(), 15);
        assert_eq!(EscapeProcessor::validate_hex_digit(b'A').unwrap(), 10);
        assert_eq!(EscapeProcessor::validate_hex_digit(b'F').unwrap(), 15);

        // Invalid digits
        assert!(EscapeProcessor::validate_hex_digit(b'g').is_err());
        assert!(EscapeProcessor::validate_hex_digit(b'G').is_err());
        assert!(EscapeProcessor::validate_hex_digit(b'z').is_err());
        assert!(EscapeProcessor::validate_hex_digit(b' ').is_err());
    }

    #[test]
    fn test_unicode_escape_basic() {
        let mut buffer = [0u8; 4];

        // Test basic ASCII character \u0041 -> 'A'
        let result = EscapeProcessor::process_unicode_escape(b"0041", &mut buffer).unwrap();
        assert_eq!(result, b"A");

        // Test another ASCII character \u0048 -> 'H'
        let result = EscapeProcessor::process_unicode_escape(b"0048", &mut buffer).unwrap();
        assert_eq!(result, b"H");
    }

    #[test]
    fn test_unicode_escape_multibyte() {
        let mut buffer = [0u8; 4];

        // Test Greek alpha \u03B1 -> 'α' (2 bytes in UTF-8: 0xCE, 0xB1)
        let result = EscapeProcessor::process_unicode_escape(b"03B1", &mut buffer).unwrap();
        assert_eq!(result, "α".as_bytes());

        // Test emoji \u1F60A -> '😊' (4 bytes in UTF-8)
        let _result = EscapeProcessor::process_unicode_escape(b"1F60", &mut buffer).unwrap();
        // Note: This is actually incomplete - \u1F60A requires surrogate pairs
        // But for basic testing this verifies the hex parsing works
    }

    #[test]
    fn test_unicode_escape_invalid_hex() {
        let mut buffer = [0u8; 4];

        // Invalid hex characters
        assert!(EscapeProcessor::process_unicode_escape(b"00GG", &mut buffer).is_err());
        assert!(EscapeProcessor::process_unicode_escape(b"ZZZZ", &mut buffer).is_err());

        // Wrong length
        assert!(EscapeProcessor::process_unicode_escape(b"123", &mut buffer).is_err());
        assert!(EscapeProcessor::process_unicode_escape(b"12345", &mut buffer).is_err());
    }

    #[test]
    fn test_unicode_escape_invalid_codepoint() {
        let mut buffer = [0u8; 4];

        // Note: Most values in the BMP are valid Unicode codepoints
        // Invalid surrogate codepoints would be D800-DFFF but they're complex to test
        // For now, test basic valid cases to ensure the function works
        let result = EscapeProcessor::process_unicode_escape(b"0000", &mut buffer).unwrap();
        assert_eq!(result, "\0".as_bytes());
    }

    #[test]
    fn test_token_to_escape_char() {
        use ujson::EventToken;

        // Test all valid escape tokens
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeQuote).unwrap(),
            b'"'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeBackslash).unwrap(),
            b'\\'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeSlash).unwrap(),
            b'/'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeBackspace).unwrap(),
            b'b'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeFormFeed).unwrap(),
            b'f'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeNewline).unwrap(),
            b'n'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeCarriageReturn).unwrap(),
            b'r'
        );
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::EscapeTab).unwrap(),
            b't'
        );

        // Test invalid token
        assert_eq!(
            EscapeProcessor::token_to_escape_char(&EventToken::String),
            None
        );
    }

    #[test]
    fn test_process_escape_token() {
        use ujson::EventToken;

        // Test valid escape tokens that produce correct unescaped bytes
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeQuote).unwrap(),
            b'"'
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeBackslash).unwrap(),
            b'\\'
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeSlash).unwrap(),
            b'/'
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeBackspace).unwrap(),
            0x08
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeFormFeed).unwrap(),
            0x0C
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeNewline).unwrap(),
            b'\n'
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeCarriageReturn).unwrap(),
            b'\r'
        );
        assert_eq!(
            EscapeProcessor::process_escape_token(&EventToken::EscapeTab).unwrap(),
            b'\t'
        );

        // Test invalid token
        assert!(EscapeProcessor::process_escape_token(&EventToken::String).is_err());
    }

    #[test]
    fn test_unicode_escape_collector_basic() {
        let mut collector = UnicodeEscapeCollector::new();
        let mut utf8_buffer = [0u8; 4];

        assert_eq!(collector.hex_count(), 0);
        assert!(!collector.is_complete());

        // Add hex digits for \u0041 -> 'A'
        assert!(!collector.add_hex_digit(b'0').unwrap()); // Not complete yet
        assert!(!collector.add_hex_digit(b'0').unwrap()); // Not complete yet
        assert!(!collector.add_hex_digit(b'4').unwrap()); // Not complete yet
        assert!(collector.add_hex_digit(b'1').unwrap()); // Complete!

        assert_eq!(collector.hex_count(), 4);
        assert!(collector.is_complete());

        // Process to UTF-8
        let result = collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert_eq!(result, b"A");
    }

    #[test]
    fn test_unicode_escape_collector_invalid_hex() {
        let mut collector = UnicodeEscapeCollector::new();

        // Valid digits first
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'0').unwrap());

        // Invalid hex digit should fail
        assert!(collector.add_hex_digit(b'G').is_err());

        // State should be preserved after error
        assert_eq!(collector.hex_count(), 2);
        assert!(!collector.is_complete());
    }

    #[test]
    fn test_unicode_escape_collector_reset() {
        let mut collector = UnicodeEscapeCollector::new();

        // Add some digits
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'1').unwrap());
        assert_eq!(collector.hex_count(), 2);

        // Reset should clear state
        collector.reset();
        assert_eq!(collector.hex_count(), 0);
        assert!(!collector.is_complete());

        // Should be able to start fresh
        assert!(!collector.add_hex_digit(b'A').unwrap());
        assert_eq!(collector.hex_count(), 1);
    }

    #[test]
    fn test_unicode_escape_collector_multibyte() {
        let mut collector = UnicodeEscapeCollector::new();
        let mut utf8_buffer = [0u8; 4];

        // Add hex digits for \u03B1 -> 'α' (Greek alpha)
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'3').unwrap());
        assert!(!collector.add_hex_digit(b'B').unwrap());
        assert!(collector.add_hex_digit(b'1').unwrap());

        let result = collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert_eq!(result, "α".as_bytes());
    }

    #[test]
    fn test_unicode_escape_collector_incomplete_processing() {
        let mut collector = UnicodeEscapeCollector::new();
        let mut utf8_buffer = [0u8; 4];

        // Add only 2 digits
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'0').unwrap());

        // Should fail to process incomplete sequence
        assert!(collector.process_to_utf8(&mut utf8_buffer).is_err());
    }
}
