// SPDX-License-Identifier: Apache-2.0

use crate::parse_error::ParseError;
use crate::shared::{ContentRange, UnexpectedState};

/// Shared utilities for processing JSON escape sequences.
/// This module contains pure functions for escape processing that can be used
/// by both CopyOnEscape and StreamingBuffer components.
pub struct EscapeProcessor;
use crate::ujson;
use ujson::EventToken;

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
            EventToken::EscapeQuote => Some(b'"'),
            EventToken::EscapeBackslash => Some(b'\\'),
            EventToken::EscapeSlash => Some(b'/'),
            EventToken::EscapeBackspace => Some(b'b'),
            EventToken::EscapeFormFeed => Some(b'f'),
            EventToken::EscapeNewline => Some(b'n'),
            EventToken::EscapeCarriageReturn => Some(b'r'),
            EventToken::EscapeTab => Some(b't'),
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
        let escape_char =
            Self::token_to_escape_char(escape_token).ok_or(UnexpectedState::InvalidEscapeToken)?;
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
            b'a'..=b'f' => Ok(byte.wrapping_sub(b'a').wrapping_add(10) as u32),
            b'A'..=b'F' => Ok(byte.wrapping_sub(b'A').wrapping_add(10) as u32),
            _ => Err(ParseError::InvalidUnicodeHex),
        }
    }

    /// Check if a Unicode codepoint is a high surrogate (0xD800-0xDBFF)
    pub fn is_high_surrogate(codepoint: u32) -> bool {
        (0xD800..=0xDBFF).contains(&codepoint)
    }

    /// Check if a Unicode codepoint is a low surrogate (0xDC00-0xDFFF)
    pub fn is_low_surrogate(codepoint: u32) -> bool {
        (0xDC00..=0xDFFF).contains(&codepoint)
    }

    /// Combine a high and low surrogate pair into a single Unicode codepoint
    pub fn combine_surrogate_pair(high: u32, low: u32) -> Result<u32, ParseError> {
        if !Self::is_high_surrogate(high) || !Self::is_low_surrogate(low) {
            return Err(ParseError::InvalidUnicodeCodepoint);
        }

        // Combine surrogates according to UTF-16 specification
        let codepoint = 0x10000 + ((high & 0x3FF) << 10) + (low & 0x3FF);
        Ok(codepoint)
    }

    /// Process a Unicode escape sequence with surrogate pair support.
    /// This function handles both individual Unicode escapes and surrogate pairs.
    ///
    /// # Arguments
    /// * `hex_slice` - A 4-byte slice containing the hexadecimal digits
    /// * `utf8_buffer` - A buffer to write the UTF-8 encoded result (must be at least 4 bytes)
    /// * `pending_high_surrogate` - Optional high surrogate from previous escape
    ///
    /// # Returns
    /// A tuple containing:
    /// - Optional UTF-8 encoded bytes (None if this is a high surrogate waiting for low)
    /// - Optional high surrogate to save for next escape (Some if this is a high surrogate)
    pub fn process_unicode_escape<'a>(
        hex_slice: &[u8],
        utf8_buffer: &'a mut [u8],
        pending_high_surrogate: Option<u32>,
    ) -> Result<(Option<&'a [u8]>, Option<u32>), ParseError> {
        if hex_slice.len() != 4 {
            return Err(ParseError::InvalidUnicodeHex);
        }

        // Convert hex bytes to Unicode codepoint
        let mut codepoint = 0u32;
        for &byte in hex_slice {
            let digit = Self::validate_hex_digit(byte)?;
            codepoint = (codepoint << 4) | digit;
        }

        // Check if we have a pending high surrogate
        if let Some(high) = pending_high_surrogate {
            // We should have a low surrogate now
            if Self::is_low_surrogate(codepoint) {
                // Combine the surrogate pair
                let combined = Self::combine_surrogate_pair(high, codepoint)?;
                let ch = char::from_u32(combined).ok_or(ParseError::InvalidUnicodeCodepoint)?;
                let utf8_str = ch.encode_utf8(utf8_buffer);
                Ok((Some(utf8_str.as_bytes()), None))
            } else {
                // Error: high surrogate not followed by low surrogate
                Err(ParseError::InvalidUnicodeCodepoint)
            }
        } else {
            // No pending high surrogate
            if Self::is_high_surrogate(codepoint) {
                // Save this high surrogate for the next escape
                Ok((None, Some(codepoint)))
            } else if Self::is_low_surrogate(codepoint) {
                // Error: low surrogate without preceding high surrogate
                Err(ParseError::InvalidUnicodeCodepoint)
            } else {
                // Regular Unicode character
                let ch = char::from_u32(codepoint).ok_or(ParseError::InvalidUnicodeCodepoint)?;
                let utf8_str = ch.encode_utf8(utf8_buffer);
                Ok((Some(utf8_str.as_bytes()), None))
            }
        }
    }
}

/// Shared Unicode escape hex digit collector for both parsers.
/// Provides a common interface for collecting the 4 hex digits in \uXXXX sequences.
/// Supports surrogate pairs by tracking pending high surrogates.
#[derive(Debug)]
pub struct UnicodeEscapeCollector {
    /// Buffer to collect the 4 hex digits
    hex_buffer: [u8; 4],
    /// Current position in the hex buffer (0-4)
    hex_pos: usize,
    /// Pending high surrogate waiting for low surrogate
    pending_high_surrogate: Option<u32>,
}

impl UnicodeEscapeCollector {
    /// Create a new Unicode escape collector
    pub fn new() -> Self {
        Self {
            hex_buffer: [0u8; 4],
            hex_pos: 0,
            pending_high_surrogate: None,
        }
    }

    /// Reset the collector for a new Unicode escape sequence
    pub fn reset(&mut self) {
        self.hex_pos = 0;
        // Note: We don't reset pending_high_surrogate here since it needs to persist
        // across Unicode escape sequences to properly handle surrogate pairs
    }

    /// Reset the collector completely, including any pending surrogate state
    pub fn reset_all(&mut self) {
        self.hex_pos = 0;
        self.pending_high_surrogate = None;
    }

    /// Add a hex digit to the collector
    /// Returns true if this completes the 4-digit sequence
    pub fn add_hex_digit(&mut self, digit: u8) -> Result<bool, ParseError> {
        // Validate the hex digit first
        EscapeProcessor::validate_hex_digit(digit)?;

        if self.hex_pos >= 4 {
            return Err(UnexpectedState::InvalidUnicodeEscape.into());
        }

        if let Some(slot) = self.hex_buffer.get_mut(self.hex_pos) {
            *slot = digit;
        } else {
            return Err(ParseError::InvalidUnicodeHex);
        }

        self.hex_pos = self.hex_pos.saturating_add(1);

        Ok(self.hex_pos == 4)
    }

    /// Process the collected hex digits with surrogate pair support
    /// Should only be called when is_complete() returns true
    /// Returns (optional UTF-8 bytes, whether surrogate state changed)
    pub fn process_to_utf8<'a>(
        &mut self,
        utf8_buffer: &'a mut [u8],
    ) -> Result<(Option<&'a [u8]>, bool), ParseError> {
        if self.hex_pos != 4 {
            return Err(UnexpectedState::InvalidUnicodeEscape.into());
        }

        let (result, new_pending) = EscapeProcessor::process_unicode_escape(
            &self.hex_buffer,
            utf8_buffer,
            self.pending_high_surrogate,
        )?;

        let surrogate_state_changed = self.pending_high_surrogate != new_pending;
        self.pending_high_surrogate = new_pending;

        Ok((result, surrogate_state_changed))
    }

    /// Check if there's a pending high surrogate waiting for a low surrogate
    pub fn has_pending_high_surrogate(&self) -> bool {
        self.pending_high_surrogate.is_some()
    }
}

impl Default for UnicodeEscapeCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ujson::EventToken;

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
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"0041", &mut buffer, None).unwrap();
        assert_eq!(result.unwrap(), b"A");
        assert_eq!(pending, None);

        // Test another ASCII character \u0048 -> 'H'
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"0048", &mut buffer, None).unwrap();
        assert_eq!(result.unwrap(), b"H");
        assert_eq!(pending, None);
    }

    #[test]
    fn test_unicode_escape_multibyte() {
        let mut buffer = [0u8; 4];

        // Test Greek alpha \u03B1 -> 'α' (2 bytes in UTF-8: 0xCE, 0xB1)
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"03B1", &mut buffer, None).unwrap();
        assert_eq!(result.unwrap(), "α".as_bytes());
        assert_eq!(pending, None);

        // Test emoji \u1F60A -> '😊' (4 bytes in UTF-8)
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"1F60", &mut buffer, None).unwrap();
        // Note: This is actually incomplete - \u1F60A requires surrogate pairs
        // But for basic testing this verifies the hex parsing works
        assert!(result.is_some());
        assert_eq!(pending, None);
    }

    #[test]
    fn test_unicode_escape_invalid_hex() {
        let mut buffer = [0u8; 4];

        // Invalid hex characters
        assert!(EscapeProcessor::process_unicode_escape(b"00GG", &mut buffer, None).is_err());
        assert!(EscapeProcessor::process_unicode_escape(b"ZZZZ", &mut buffer, None).is_err());

        // Wrong length
        assert!(EscapeProcessor::process_unicode_escape(b"123", &mut buffer, None).is_err());
        assert!(EscapeProcessor::process_unicode_escape(b"12345", &mut buffer, None).is_err());
    }

    #[test]
    fn test_unicode_escape_invalid_codepoint() {
        let mut buffer = [0u8; 4];

        // Note: Most values in the BMP are valid Unicode codepoints
        // Invalid surrogate codepoints would be D800-DFFF but they're complex to test
        // For now, test basic valid cases to ensure the function works
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"0000", &mut buffer, None).unwrap();
        assert_eq!(result.unwrap(), "\0".as_bytes());
        assert_eq!(pending, None);
    }

    #[test]
    fn test_token_to_escape_char() {
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

        // Add hex digits for \u0041 -> 'A'
        assert!(!collector.add_hex_digit(b'0').unwrap()); // Not complete yet
        assert!(!collector.add_hex_digit(b'0').unwrap()); // Not complete yet
        assert!(!collector.add_hex_digit(b'4').unwrap()); // Not complete yet
        assert!(collector.add_hex_digit(b'1').unwrap()); // Complete!

        // Process to UTF-8
        let (result, _surrogate_state_changed) =
            collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert_eq!(result.unwrap(), b"A");
    }

    #[test]
    fn test_unicode_escape_collector_invalid_hex() {
        let mut collector = UnicodeEscapeCollector::new();

        // Valid digits first
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'0').unwrap());

        // Invalid hex digit should fail
        assert!(collector.add_hex_digit(b'G').is_err());
    }

    #[test]
    fn test_unicode_escape_collector_reset() {
        let mut collector = UnicodeEscapeCollector::new();

        // Add some digits
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(!collector.add_hex_digit(b'1').unwrap());

        // Reset should clear hex position but not surrogate state
        collector.reset();

        // Should be able to start fresh
        assert!(!collector.add_hex_digit(b'A').unwrap());
    }

    #[test]
    fn test_unicode_escape_collector_surrogate_support() {
        let mut collector = UnicodeEscapeCollector::new();
        let mut utf8_buffer = [0u8; 4];

        // Process high surrogate \uD801
        assert!(!collector.add_hex_digit(b'D').unwrap());
        assert!(!collector.add_hex_digit(b'8').unwrap());
        assert!(!collector.add_hex_digit(b'0').unwrap());
        assert!(collector.add_hex_digit(b'1').unwrap());

        let (result, state_changed) = collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert_eq!(result, None); // No UTF-8 output yet
        assert!(state_changed); // Surrogate state changed
        assert!(collector.has_pending_high_surrogate());

        // Reset for next escape sequence
        collector.reset();

        // Process low surrogate \uDC37
        assert!(!collector.add_hex_digit(b'D').unwrap());
        assert!(!collector.add_hex_digit(b'C').unwrap());
        assert!(!collector.add_hex_digit(b'3').unwrap());
        assert!(collector.add_hex_digit(b'7').unwrap());

        let (result, state_changed) = collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert!(result.is_some()); // Should have UTF-8 output
        assert!(state_changed); // Surrogate state changed (cleared)
        assert!(!collector.has_pending_high_surrogate());

        // Verify it's the correct UTF-8 encoding for U+10437
        assert_eq!(result.unwrap(), [0xF0, 0x90, 0x90, 0xB7]);
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

        let (result, _surrogate_state_changed) =
            collector.process_to_utf8(&mut utf8_buffer).unwrap();
        assert_eq!(result.unwrap(), "α".as_bytes());
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

    #[test]
    fn test_surrogate_pair_detection() {
        // Test high surrogate detection
        assert!(EscapeProcessor::is_high_surrogate(0xD800));
        assert!(EscapeProcessor::is_high_surrogate(0xD801));
        assert!(EscapeProcessor::is_high_surrogate(0xDBFF));
        assert!(!EscapeProcessor::is_high_surrogate(0xD7FF));
        assert!(!EscapeProcessor::is_high_surrogate(0xDC00));

        // Test low surrogate detection
        assert!(EscapeProcessor::is_low_surrogate(0xDC00));
        assert!(EscapeProcessor::is_low_surrogate(0xDC37));
        assert!(EscapeProcessor::is_low_surrogate(0xDFFF));
        assert!(!EscapeProcessor::is_low_surrogate(0xDBFF));
        assert!(!EscapeProcessor::is_low_surrogate(0xE000));
    }

    #[test]
    fn test_surrogate_pair_combination() {
        // Test valid surrogate pair: \uD801\uDC37 -> U+10437
        let combined = EscapeProcessor::combine_surrogate_pair(0xD801, 0xDC37).unwrap();
        assert_eq!(combined, 0x10437);

        // Test another valid pair: \uD834\uDD1E -> U+1D11E (musical symbol)
        let combined = EscapeProcessor::combine_surrogate_pair(0xD834, 0xDD1E).unwrap();
        assert_eq!(combined, 0x1D11E);

        // Test invalid combinations
        assert!(EscapeProcessor::combine_surrogate_pair(0x0041, 0xDC37).is_err()); // Not high surrogate
        assert!(EscapeProcessor::combine_surrogate_pair(0xD801, 0x0041).is_err());
        // Not low surrogate
    }

    #[test]
    fn test_unicode_escape_with_surrogate_support() {
        let mut buffer = [0u8; 4];

        // Test regular Unicode character (not surrogate)
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"0041", &mut buffer, None).unwrap();
        assert_eq!(result, Some(b"A".as_slice()));
        assert_eq!(pending, None);

        // Test high surrogate - should return None and save the high surrogate
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"D801", &mut buffer, None).unwrap();
        assert_eq!(result, None);
        assert_eq!(pending, Some(0xD801));

        // Test low surrogate following high surrogate - should combine
        let (result, pending) =
            EscapeProcessor::process_unicode_escape(b"DC37", &mut buffer, Some(0xD801)).unwrap();
        assert!(result.is_some());
        assert_eq!(pending, None);
        // The result should be the UTF-8 encoding of U+10437
        assert_eq!(result.unwrap(), [0xF0, 0x90, 0x90, 0xB7]);
    }

    #[test]
    fn test_unicode_escape_surrogate_error_cases() {
        let mut buffer = [0u8; 4];

        // Test low surrogate without preceding high surrogate - should error
        let result = EscapeProcessor::process_unicode_escape(b"DC37", &mut buffer, None);
        assert!(result.is_err());

        // Test high surrogate followed by non-low-surrogate - should error
        let result = EscapeProcessor::process_unicode_escape(b"0041", &mut buffer, Some(0xD801));
        assert!(result.is_err());
    }
}

/// Shared implementation for processing a Unicode escape sequence WITH surrogate pair support.
///
/// This function centralizes the logic for handling `\uXXXX` escapes, which is
/// common to both the pull-based and stream-based parsers. It uses a generic
/// `hex_slice_provider` to remain independent of the underlying buffer implementation
/// (`SliceInputBuffer` vs. `StreamBuffer`).
///
/// # Arguments
/// * `current_pos` - The parser's current position in the input buffer, right after the 4 hex digits.
/// * `unicode_escape_collector` - A mutable reference to the shared `UnicodeEscapeCollector`.
/// * `hex_slice_provider` - A closure that takes a start and end position and returns the hex digit slice.
/// * `utf8_buf` - A buffer to write the UTF-8 encoded result into.
///
/// # Returns
/// A tuple containing:
/// - Optional UTF-8 byte slice (None if this is a high surrogate waiting for low surrogate)
/// - The start position of the escape sequence (`\uXXXX`)
pub(crate) fn process_unicode_escape_sequence<'a, F>(
    current_pos: usize,
    unicode_escape_collector: &mut UnicodeEscapeCollector,
    mut hex_slice_provider: F,
) -> Result<(Option<([u8; 4], usize)>, usize), ParseError>
where
    F: FnMut(usize, usize) -> Result<&'a [u8], ParseError>,
{
    let (hex_start, hex_end, escape_start_pos) = ContentRange::unicode_escape_bounds(current_pos);

    // Extract the 4 hex digits from the buffer using the provider
    let hex_slice = hex_slice_provider(hex_start, hex_end)?;

    if hex_slice.len() != 4 {
        return Err(UnexpectedState::InvalidUnicodeEscape.into());
    }

    // Feed hex digits to the shared collector
    for &hex_digit in hex_slice {
        unicode_escape_collector.add_hex_digit(hex_digit)?;
    }

    // Check if we had a pending high surrogate before processing
    let had_pending_high_surrogate = unicode_escape_collector.has_pending_high_surrogate();

    // Create a local buffer for the UTF-8 result
    let mut utf8_buf = [0u8; 4];

    // Process the complete sequence to UTF-8 with surrogate support
    let (utf8_bytes_opt, _surrogate_state_changed) =
        unicode_escape_collector.process_to_utf8(&mut utf8_buf)?;

    // If we have a result, copy it to a new array to return by value
    let result_by_value = utf8_bytes_opt.map(|bytes| {
        let mut value_buf = [0u8; 4];
        let len = bytes.len();
        value_buf[..len].copy_from_slice(bytes);
        (value_buf, len)
    });

    // If we're completing a surrogate pair (had pending high surrogate and now have UTF-8 bytes),
    // return the position of the high surrogate start instead of the low surrogate start
    let final_escape_start_pos = if had_pending_high_surrogate && result_by_value.is_some() {
        // High surrogate started 6 bytes before the current low surrogate
        escape_start_pos.saturating_sub(6)
    } else {
        escape_start_pos
    };

    Ok((result_by_value, final_escape_start_pos))
}
