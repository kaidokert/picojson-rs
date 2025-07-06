// SPDX-License-Identifier: Apache-2.0

use crate::parse_error::ParseError;
use crate::{shared::UnexpectedState, String};

/// A struct that encapsulates copy-on-escape string processing with full buffer ownership.
///
/// This version owns the scratch buffer for the entire parser lifetime, eliminating
/// borrow checker issues. The buffer is reused across multiple string operations
/// via reset() calls.
pub struct CopyOnEscape<'a, 'b> {
    /// Reference to the input data being parsed
    input: &'a [u8],
    /// Owned mutable reference to the scratch buffer for unescaping
    scratch: &'b mut [u8],
    /// Global position in the scratch buffer (never resets)
    global_scratch_pos: usize,

    // Current string processing state (resets per string)
    /// Where the current string started in the input
    string_start: usize,
    /// Position in input where we last copied from (for span copying)
    last_copied_pos: usize,
    /// Whether we've encountered any escapes (and thus are using scratch buffer)
    using_scratch: bool,
    /// Starting position in scratch buffer for this string
    scratch_start: usize,
    /// Current position in scratch buffer for this string
    scratch_pos: usize,
}

impl<'a, 'b> CopyOnEscape<'a, 'b> {
    /// Creates a new CopyOnEscape processor with full buffer ownership.
    ///
    /// # Arguments
    /// * `input` - The input byte slice being parsed
    /// * `scratch` - Mutable scratch buffer for escape processing (owned for parser lifetime)
    pub fn new(input: &'a [u8], scratch: &'b mut [u8]) -> Self {
        Self {
            input,
            scratch,
            global_scratch_pos: 0,
            string_start: 0,
            last_copied_pos: 0,
            using_scratch: false,
            scratch_start: 0,
            scratch_pos: 0,
        }
    }

    /// Resets the processor for a new string at the given position.
    /// The scratch buffer position continues from where previous strings left off.
    ///
    /// # Arguments
    /// * `pos` - Position in input where the string content starts
    pub fn begin_string(&mut self, pos: usize) {
        self.string_start = pos;
        self.last_copied_pos = pos;
        self.using_scratch = false; // Start with zero-copy optimization
        self.scratch_start = self.global_scratch_pos;
        self.scratch_pos = self.global_scratch_pos;
    }

    /// Copies a span from last_copied_pos to end position with bounds checking.
    ///
    /// # Arguments
    /// * `end` - End position in input (exclusive)
    /// * `extra_space` - Additional space needed beyond the span (e.g., for escape character)
    fn copy_span_to_scratch(&mut self, end: usize, extra_space: usize) -> Result<(), ParseError> {
        if end > self.last_copied_pos {
            let span = self
                .input
                .get(self.last_copied_pos..end)
                .ok_or(UnexpectedState::InvalidSliceBounds)?;
            let end_pos = self
                .scratch_pos
                .checked_add(span.len())
                .ok_or(ParseError::NumericOverflow)?;
            if end_pos
                .checked_add(extra_space)
                .ok_or(ParseError::NumericOverflow)?
                > self.scratch.len()
            {
                return Err(ParseError::ScratchBufferFull);
            }
            // Use zip to avoid copy_from_slice panic checks
            if let Some(scratch_slice) = self.scratch.get_mut(self.scratch_pos..end_pos) {
                for (dst, &src) in scratch_slice.iter_mut().zip(span.iter()) {
                    *dst = src;
                }
            }
            self.scratch_pos = self.scratch_pos.saturating_add(span.len());
        }
        Ok(())
    }

    /// Handles an escape sequence at the given position.
    ///
    /// This triggers copy-on-escape if this is the first escape encountered.
    /// For subsequent escapes, it continues the unescaping process.
    ///
    /// # Arguments
    /// * `pos` - Current position in input (pointing just after the escape sequence)
    /// * `unescaped_char` - The unescaped character to write to scratch buffer
    pub fn handle_escape(&mut self, pos: usize, unescaped_char: u8) -> Result<(), ParseError> {
        if !self.using_scratch {
            // First escape found - trigger copy-on-escape
            self.using_scratch = true;
        }

        // Copy the span from last_copied_pos to the backslash position
        // The backslash is at pos-2 (since pos points after the escape sequence)
        let backslash_pos = pos.saturating_sub(2);
        self.copy_span_to_scratch(backslash_pos, 1)?;

        // Write the unescaped character
        if self.scratch_pos >= self.scratch.len() {
            return Err(ParseError::ScratchBufferFull);
        }
        if let Some(slot) = self.scratch.get_mut(self.scratch_pos) {
            *slot = unescaped_char;
        } else {
            return Err(ParseError::ScratchBufferFull);
        }
        self.scratch_pos = self.scratch_pos.saturating_add(1);

        // Update last copied position to after the escape sequence
        self.last_copied_pos = pos;

        Ok(())
    }

    /// Handles a Unicode escape sequence by writing the UTF-8 encoded bytes to scratch buffer.
    ///
    /// This triggers copy-on-escape if this is the first escape encountered.
    /// Unicode escapes span 6 bytes in input (\uXXXX) but produce 1-4 bytes of UTF-8 output.
    ///
    /// # Arguments
    /// * `start_pos` - Position in input where the \uXXXX sequence starts (at the backslash)
    /// * `utf8_bytes` - The UTF-8 encoded bytes to write (1-4 bytes)
    pub fn handle_unicode_escape(
        &mut self,
        start_pos: usize,
        utf8_bytes: &[u8],
    ) -> Result<(), ParseError> {
        if !self.using_scratch {
            // First escape found - trigger copy-on-escape
            self.using_scratch = true;
        }

        // Copy the span from last_copied_pos to the backslash position
        self.copy_span_to_scratch(start_pos, utf8_bytes.len())?;

        // Write the UTF-8 encoded bytes
        let new_scratch_pos = self
            .scratch_pos
            .checked_add(utf8_bytes.len())
            .ok_or(ParseError::NumericOverflow)?;
        if new_scratch_pos > self.scratch.len() {
            return Err(ParseError::ScratchBufferFull);
        }
        // Use zip to avoid copy_from_slice panic checks
        if let Some(scratch_slice) = self.scratch.get_mut(self.scratch_pos..new_scratch_pos) {
            for (dst, &src) in scratch_slice.iter_mut().zip(utf8_bytes.iter()) {
                *dst = src;
            }
        }
        self.scratch_pos = self.scratch_pos.saturating_add(utf8_bytes.len());

        // Update last copied position to after the 6-byte Unicode escape sequence
        self.last_copied_pos = start_pos.saturating_add(6); // \uXXXX is always 6 bytes

        Ok(())
    }

    /// Completes string processing and returns the final String.
    /// Updates the global scratch position for the next string.
    ///
    /// # Arguments
    /// * `pos` - Position in input where the string ends
    ///
    /// # Returns
    /// The final String (either borrowed or unescaped)
    pub fn end_string(&mut self, pos: usize) -> Result<String<'_, '_>, ParseError> {
        if self.using_scratch {
            // Copy final span from last_copied_pos to end
            self.copy_span_to_scratch(pos, 0)?;
            // Update global position for next string
            self.global_scratch_pos = self.scratch_pos;

            // Return unescaped string from scratch buffer
            let unescaped_slice = self
                .scratch
                .get(self.scratch_start..self.scratch_pos)
                .ok_or(UnexpectedState::InvalidSliceBounds)?;
            let unescaped_str = crate::shared::from_utf8(unescaped_slice)?;
            Ok(String::Unescaped(unescaped_str))
        } else {
            // No escapes found - return borrowed slice (zero-copy!)
            let borrowed_bytes = self
                .input
                .get(self.string_start..pos)
                .ok_or(UnexpectedState::InvalidSliceBounds)?;
            let borrowed_str = crate::shared::from_utf8(borrowed_bytes)?;
            Ok(String::Borrowed(borrowed_str))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coe2_no_escapes() {
        let input = b"hello world";
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        let result = processor.end_string(input.len()).unwrap();

        // Should return borrowed (zero-copy)
        assert!(matches!(result, String::Borrowed("hello world")));
    }

    #[test]
    fn test_coe2_with_escapes() {
        let input = b"hello\\nworld";
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        processor.handle_escape(7, b'\n').unwrap(); // Position after "hello\n"
        let result = processor.end_string(input.len()).unwrap();

        // Should return unescaped
        assert!(matches!(result, String::Unescaped(s) if s == "hello\nworld"));
    }

    #[test]
    fn test_coe2_multiple_strings() {
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(b"dummy", &mut scratch);

        // First string with escapes
        let input1 = b"first\\tstring";
        processor.input = input1;
        processor.begin_string(0);
        processor.handle_escape(7, b'\t').unwrap(); // After "first\t"
        let result1 = processor.end_string(input1.len()).unwrap();

        assert!(matches!(result1, String::Unescaped(s) if s == "first\tstring"));

        // Second string without escapes
        let input2 = b"second string";
        processor.input = input2;
        processor.begin_string(0);
        let result2 = processor.end_string(input2.len()).unwrap();

        // Should be borrowed (no scratch used)
        assert!(matches!(result2, String::Borrowed("second string")));

        // Third string with escapes
        let input3 = b"third\\nstring";
        processor.input = input3;
        processor.begin_string(0);
        processor.handle_escape(7, b'\n').unwrap();
        let result3 = processor.end_string(input3.len()).unwrap();

        assert!(matches!(result3, String::Unescaped(s) if s == "third\nstring"));
    }

    #[test]
    fn test_coe2_multiple_escapes() {
        let input = b"a\\nb\\tc";
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        processor.handle_escape(3, b'\n').unwrap(); // After "a\n"
        processor.handle_escape(6, b'\t').unwrap(); // After "b\t"
        let result = processor.end_string(input.len()).unwrap();

        assert!(matches!(result, String::Unescaped(s) if s == "a\nb\tc"));
    }

    #[test]
    fn test_coe2_buffer_reuse() {
        let mut scratch = [0u8; 50]; // Larger buffer
        let mut processor = CopyOnEscape::new(b"dummy", &mut scratch);

        // Fill up buffer with first string
        let input1 = b"long\\tstring\\nwith\\rescapes";
        processor.input = input1;
        processor.begin_string(0);
        processor.handle_escape(6, b'\t').unwrap();
        processor.handle_escape(14, b'\n').unwrap();
        processor.handle_escape(20, b'\r').unwrap();
        let result1 = processor.end_string(input1.len()).unwrap();

        assert!(matches!(result1, String::Unescaped(_)));

        // Use buffer for second string (will use remaining space)
        let input2 = b"new\\tstring";
        processor.input = input2;
        processor.begin_string(0);
        processor.handle_escape(5, b'\t').unwrap();
        let result2 = processor.end_string(input2.len()).unwrap();

        assert!(matches!(result2, String::Unescaped(s) if s == "new\tstring"));
    }

    #[test]
    fn test_coe2_buffer_full() {
        let input = b"very long string with escape\\n";
        let mut scratch = [0u8; 5]; // Intentionally small
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        let result = processor.handle_escape(30, b'\n');

        assert!(matches!(result, Err(ParseError::ScratchBufferFull)));
    }

    #[test]
    fn test_coe2_unicode_escape() {
        let input = b"hello\\u0041world"; // \u0041 = 'A'
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        // Unicode escape: \u0041 -> UTF-8 'A' (1 byte)
        let utf8_a = b"A";
        processor.handle_unicode_escape(5, utf8_a).unwrap(); // Position at backslash
        let result = processor.end_string(input.len()).unwrap();

        // Should return unescaped with 'A' substituted
        assert!(matches!(result, String::Unescaped(s) if s == "helloAworld"));
    }

    #[test]
    fn test_coe2_unicode_escape_multibyte() {
        let input = b"test\\u03B1end"; // \u03B1 = Greek alpha 'α' (2 bytes in UTF-8)
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        // Unicode escape: \u03B1 -> UTF-8 'α' (2 bytes: 0xCE, 0xB1)
        let utf8_alpha = "α".as_bytes(); // UTF-8 encoding of Greek alpha
        processor.handle_unicode_escape(4, utf8_alpha).unwrap(); // Position at backslash
        let result = processor.end_string(input.len()).unwrap();

        // Should return unescaped with 'α' substituted
        assert!(matches!(result, String::Unescaped(s) if s == "testαend"));
    }

    #[test]
    fn test_coe2_unicode_escape_no_prior_escapes() {
        let input = b"plain\\u0041"; // \u0041 = 'A'
        let mut scratch = [0u8; 100];
        let mut processor = CopyOnEscape::new(input, &mut scratch);

        processor.begin_string(0);
        // Should trigger copy-on-escape since this is first escape
        let utf8_a = b"A";
        processor.handle_unicode_escape(5, utf8_a).unwrap();
        let result = processor.end_string(input.len()).unwrap();

        assert!(matches!(result, String::Unescaped(s) if s == "plainA"));
    }
}
