// SPDX-License-Identifier: Apache-2.0

use crate::ParseError;

/// Error types for DirectBuffer operations
#[derive(Debug, PartialEq)]
pub enum DirectBufferError {
    /// Buffer is full and cannot accommodate more data
    BufferFull,
    /// Attempted to read beyond available data
    EndOfData,
    /// Invalid buffer state or operation
    InvalidState(&'static str),
}

impl From<DirectBufferError> for ParseError {
    fn from(err: DirectBufferError) -> Self {
        match err {
            DirectBufferError::BufferFull => ParseError::ScratchBufferFull,
            DirectBufferError::EndOfData => ParseError::EndOfData,
            DirectBufferError::InvalidState(msg) => ParseError::UnexpectedState(msg),
        }
    }
}

/// DirectBuffer manages a single buffer for both input and escape processing
///
/// Key design principles:
/// - Reader fills unused portions of buffer directly
/// - Unescaped content is copied to buffer start when needed
/// - Zero-copy string extraction when no escapes are present
/// - Guaranteed space for escape processing (unescaped ≤ escaped)
pub struct DirectBuffer<'a> {
    /// The entire buffer slice
    buffer: &'a mut [u8],
    /// Current position where tokenizer is reading
    tokenize_pos: usize,
    /// End of valid data from Reader (buffer[0..data_end] contains valid data)
    data_end: usize,
    /// Length of unescaped content at buffer start (0 if no unescaping active)
    unescaped_len: usize,
    /// Minimum space to reserve for escape processing
    escape_reserve: usize,
}

impl<'a> DirectBuffer<'a> {
    /// Create a new DirectBuffer with the given buffer slice
    pub fn new(buffer: &'a mut [u8]) -> Self {
        // Reserve 10% of buffer for escape processing, minimum 64 bytes
        let escape_reserve = (buffer.len() / 10).max(64);

        Self {
            buffer,
            tokenize_pos: 0,
            data_end: 0,
            unescaped_len: 0,
            escape_reserve,
        }
    }

    /// Get the current byte at tokenize position
    pub fn current_byte(&self) -> Result<u8, DirectBufferError> {
        if self.tokenize_pos >= self.data_end {
            return Err(DirectBufferError::EndOfData);
        }
        Ok(self.buffer[self.tokenize_pos])
    }

    /// Advance the tokenize position by one byte
    pub fn advance(&mut self) -> Result<(), DirectBufferError> {
        if self.tokenize_pos >= self.data_end {
            return Err(DirectBufferError::EndOfData);
        }
        self.tokenize_pos += 1;
        Ok(())
    }

    /// Get remaining bytes available for reading
    pub fn remaining_bytes(&self) -> usize {
        self.data_end.saturating_sub(self.tokenize_pos)
    }

    /// Get slice for Reader to fill with new data
    /// Returns None if no space available
    pub fn get_fill_slice(&mut self) -> Option<&mut [u8]> {
        if self.data_end >= self.buffer.len() {
            return None;
        }
        Some(&mut self.buffer[self.data_end..])
    }

    /// Mark that Reader filled `bytes_read` bytes
    pub fn mark_filled(&mut self, bytes_read: usize) -> Result<(), DirectBufferError> {
        if self.data_end + bytes_read > self.buffer.len() {
            return Err(DirectBufferError::InvalidState(
                "Attempted to mark more bytes than buffer space",
            ));
        }
        self.data_end += bytes_read;
        Ok(())
    }

    /// Start unescaping and copy existing content from a range in the buffer
    /// This handles the common case of starting escape processing partway through a string
    pub fn start_unescaping_with_copy(
        &mut self,
        max_escaped_len: usize,
        copy_start: usize,
        copy_end: usize,
    ) -> Result<(), DirectBufferError> {
        // Clear any previous unescaped content
        self.unescaped_len = 0;

        // Ensure we have space at the start for unescaping
        if max_escaped_len > self.buffer.len() {
            return Err(DirectBufferError::BufferFull);
        }

        // Copy existing content if there is any
        if copy_end > copy_start && copy_start < self.data_end {
            let span_len = copy_end - copy_start;

            // Ensure the span fits in the buffer - return error instead of silent truncation
            if span_len > self.buffer.len() {
                return Err(DirectBufferError::BufferFull);
            }

            // Copy within the same buffer: move data from [copy_start..copy_end] to [0..span_len]
            // Use copy_within to handle overlapping ranges safely
            self.buffer
                .copy_within(copy_start..copy_start + span_len, 0);
            self.unescaped_len = span_len;
        }

        Ok(())
    }

    /// Get the unescaped content slice
    pub fn get_unescaped_slice(&self) -> Result<&[u8], DirectBufferError> {
        if self.unescaped_len == 0 {
            return Err(DirectBufferError::InvalidState(
                "No unescaped content available",
            ));
        }
        Ok(&self.buffer[0..self.unescaped_len])
    }

    /// Clear unescaped content (call after yielding unescaped string)
    pub fn clear_unescaped(&mut self) {
        self.unescaped_len = 0;
    }

    /// Get current tokenize position (for string start tracking)
    pub fn current_position(&self) -> usize {
        self.tokenize_pos
    }

    /// Check if buffer is empty (no more data to process)
    pub fn is_empty(&self) -> bool {
        self.tokenize_pos >= self.data_end
    }

    /// Check if we have unescaped content ready
    pub fn has_unescaped_content(&self) -> bool {
        self.unescaped_len > 0
    }

    /// Append a single byte to the unescaped content
    pub fn append_unescaped_byte(&mut self, byte: u8) -> Result<(), DirectBufferError> {
        let available_space = self.buffer.len().saturating_sub(self.escape_reserve);
        if self.unescaped_len >= available_space {
            return Err(DirectBufferError::BufferFull);
        }

        self.buffer[self.unescaped_len] = byte;
        self.unescaped_len += 1;
        Ok(())
    }

    /// Get a string slice from the buffer (zero-copy)
    /// Used for strings without escapes
    pub fn get_string_slice(&self, start: usize, end: usize) -> Result<&[u8], DirectBufferError> {
        if start > end || end > self.data_end {
            return Err(DirectBufferError::InvalidState("Invalid slice bounds"));
        }
        Ok(&self.buffer[start..end])
    }

    /// Get buffer statistics for debugging
    pub fn stats(&self) -> DirectBufferStats {
        DirectBufferStats {
            total_capacity: self.buffer.len(),
            tokenize_pos: self.tokenize_pos,
            data_end: self.data_end,
            unescaped_len: self.unescaped_len,
            remaining_bytes: self.remaining_bytes(),
            available_space: self.buffer.len().saturating_sub(self.data_end),
            escape_reserve: self.escape_reserve,
        }
    }
}

/// Statistics for DirectBuffer state (useful for debugging and testing)
#[derive(Debug, PartialEq)]
pub struct DirectBufferStats {
    pub total_capacity: usize,
    pub tokenize_pos: usize,
    pub data_end: usize,
    pub unescaped_len: usize,
    pub remaining_bytes: usize,
    pub available_space: usize,
    pub escape_reserve: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifetime_expectations() {
        // This test demonstrates how DirectBuffer lifetimes should work
        let mut buffer = [0u8; 100];
        let mut direct_buffer = DirectBuffer::new(&mut buffer);

        // Simulate some data being in the buffer
        let test_data = b"hello world";
        direct_buffer.buffer[0..test_data.len()].copy_from_slice(test_data);
        direct_buffer.data_end = test_data.len();

        // Test that we can get buffer data

        // Test unescaped content - add some unescaped data
        direct_buffer.unescaped_len = 3;
        direct_buffer.buffer[0..3].copy_from_slice(b"abc");

        let unescaped_slice = direct_buffer.get_unescaped_slice().unwrap();
        assert_eq!(unescaped_slice, b"abc");

        // The key expectation: these slices should live as long as the original buffer
        // and be usable to create String::Borrowed(&'buffer str) and String::Unescaped(&'buffer str)
    }

    #[test]
    fn test_new_direct_buffer() {
        let mut buffer = [0u8; 100];
        let db = DirectBuffer::new(&mut buffer);

        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 0);
        assert_eq!(db.unescaped_len, 0);
        assert_eq!(db.escape_reserve, 64); // 10% of 100, minimum 64
        assert!(db.is_empty());
    }

    #[test]
    fn test_fill_and_advance() {
        let mut buffer = [0u8; 100];
        let mut db = DirectBuffer::new(&mut buffer);

        // Fill with some data
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice[0..5].copy_from_slice(b"hello");
        }
        db.mark_filled(5).unwrap();

        assert_eq!(db.data_end, 5);
        assert_eq!(db.remaining_bytes(), 5);

        // Read bytes
        assert_eq!(db.current_byte().unwrap(), b'h');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'e');
        assert_eq!(db.remaining_bytes(), 4);
    }

    #[test]
    fn test_error_conditions() {
        let mut buffer = [0u8; 10];
        let mut db = DirectBuffer::new(&mut buffer);

        // EndOfData errors
        assert_eq!(db.current_byte().unwrap_err(), DirectBufferError::EndOfData);
        assert_eq!(db.advance().unwrap_err(), DirectBufferError::EndOfData);

        // No unescaped content
        assert!(db.get_unescaped_slice().is_err());
    }

    #[test]
    fn test_buffer_stats() {
        let mut buffer = [0u8; 100];
        let mut db = DirectBuffer::new(&mut buffer);

        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice[0..10].copy_from_slice(b"0123456789");
        }
        db.mark_filled(10).unwrap();

        for _ in 0..3 {
            db.advance().unwrap();
        }

        let stats = db.stats();
        assert_eq!(stats.total_capacity, 100);
        assert_eq!(stats.tokenize_pos, 3);
        assert_eq!(stats.data_end, 10);
        assert_eq!(stats.remaining_bytes, 7);
        assert_eq!(stats.available_space, 90);
    }

    #[test]
    fn test_buffer_full_scenario() {
        // Test what happens when buffer gets completely full
        let mut buffer = [0u8; 10];
        let mut db = DirectBuffer::new(&mut buffer);

        // Fill buffer completely
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"0123456789");
        }
        db.mark_filled(10).unwrap();

        // No more space for filling
        assert!(db.get_fill_slice().is_none());

        // We can still read from buffer
        assert_eq!(db.current_byte().unwrap(), b'0');
        assert_eq!(db.remaining_bytes(), 10);
    }

    #[test]
    fn test_minimal_buffer_with_long_token() {
        // Test very small buffer with a token that doesn't fit
        let mut buffer = [0u8; 8]; // Very small buffer
        let mut db = DirectBuffer::new(&mut buffer);

        // Try to put a string that's almost as big as the buffer
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice[0..6].copy_from_slice(b"\"hello"); // Start of a long string, no closing quote
        }
        db.mark_filled(6).unwrap();

        // Advance through the data
        for _ in 0..6 {
            db.advance().unwrap();
        }

        // Now buffer is exhausted but we don't have a complete token
        assert!(db.is_empty());
        assert_eq!(db.remaining_bytes(), 0);

        // This simulates the scenario where we need more data but can't fit it
        // The parser would need to handle this by buffering the incomplete token
    }

    #[test]
    fn test_reader_returns_zero_bytes() {
        let mut buffer = [0u8; 20];
        let mut db = DirectBuffer::new(&mut buffer);

        // Simulate Reader returning 0 bytes (EOF)
        {
            let fill_slice = db.get_fill_slice().unwrap();
            assert_eq!(fill_slice.len(), 20);
            // Reader returns 0 bytes - simulating EOF or no data available
        }
        db.mark_filled(0).unwrap(); // Reader returned 0

        assert!(db.is_empty());
        assert_eq!(db.data_end, 0);
        assert_eq!(db.remaining_bytes(), 0);

        // Should still be able to get fill slice for next attempt
        let fill_slice = db.get_fill_slice().unwrap();
        assert_eq!(fill_slice.len(), 20);
    }

    #[test]
    fn test_maximum_escape_reserve_scenario() {
        let mut buffer = [0u8; 100];
        let db = DirectBuffer::new(&mut buffer);

        // Check escape reserve calculation
        let stats = db.stats();
        assert_eq!(stats.escape_reserve, 64); // max(100/10, 64) = 64

        // Test with smaller buffer
        let mut small_buffer = [0u8; 50];
        let small_db = DirectBuffer::new(&mut small_buffer);
        let small_stats = small_db.stats();
        assert_eq!(small_stats.escape_reserve, 64); // Still 64 (minimum)

        // Test with larger buffer
        let mut large_buffer = [0u8; 1000];
        let large_db = DirectBuffer::new(&mut large_buffer);
        let large_stats = large_db.stats();
        assert_eq!(large_stats.escape_reserve, 100); // 1000/10 = 100
    }

    #[test]
    fn test_boundary_conditions() {
        let mut buffer = [0u8; 3]; // Absolute minimum
        let mut db = DirectBuffer::new(&mut buffer);

        // Can't even hold a proper JSON token, but should not crash
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"\"a\"");
        }
        db.mark_filled(3).unwrap();

        // Should be able to read through it
        assert_eq!(db.current_byte().unwrap(), b'"');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'a');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'"');
        db.advance().unwrap();

        assert!(db.is_empty());
    }

    #[test]
    fn test_start_unescaping_with_copy_span_too_large() {
        let mut buffer = [0u8; 10]; // Small buffer
        let mut db = DirectBuffer::new(&mut buffer);

        // Fill buffer with some data
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"0123456789");
        }
        db.mark_filled(10).unwrap();

        // Try to copy a span that's larger than the entire buffer
        let copy_start = 0;
        let copy_end = 15; // This span (15 bytes) is larger than buffer (10 bytes)
        let max_escaped_len = 5; // This is fine

        // Should return BufferFull error instead of silently truncating
        let result = db.start_unescaping_with_copy(max_escaped_len, copy_start, copy_end);
        assert_eq!(result.unwrap_err(), DirectBufferError::BufferFull);

        // Test boundary case: span exactly equals buffer size should work
        let copy_end_exact = 10; // Span of exactly 10 bytes (buffer size)
        let result = db.start_unescaping_with_copy(max_escaped_len, 0, copy_end_exact);
        assert!(result.is_ok());
        assert_eq!(db.unescaped_len, 10);

        // Test valid smaller span should work
        db.clear_unescaped();
        let result = db.start_unescaping_with_copy(max_escaped_len, 2, 6); // 4 byte span
        assert!(result.is_ok());
        assert_eq!(db.unescaped_len, 4);
        assert_eq!(db.get_unescaped_slice().unwrap(), b"2345");
    }

    #[test]
    fn test_append_unescaped_byte_respects_escape_reserve() {
        let mut buffer = [0u8; 100]; // 100 byte buffer
        let mut db = DirectBuffer::new(&mut buffer);

        // Check escape reserve was set correctly (10% of 100, minimum 64)
        let stats = db.stats();
        assert_eq!(stats.escape_reserve, 64);

        // Should be able to append up to (buffer_len - escape_reserve) bytes
        let max_unescaped = 100 - db.escape_reserve; // 100 - 64 = 36

        // Fill up to the limit - should succeed
        for i in 0..max_unescaped {
            let result = db.append_unescaped_byte(b'A');
            assert!(result.is_ok(), "Failed at byte {}", i);
        }

        assert_eq!(db.unescaped_len, max_unescaped);

        // One more byte should fail due to escape reserve constraint
        let result = db.append_unescaped_byte(b'B');
        assert_eq!(result.unwrap_err(), DirectBufferError::BufferFull);

        // Verify we didn't exceed the escape reserve boundary
        assert_eq!(db.unescaped_len, max_unescaped);
    }

    #[test]
    fn test_append_unescaped_byte_escape_reserve_larger_than_buffer() {
        let mut buffer = [0u8; 10]; // Very small buffer
        let mut db = DirectBuffer::new(&mut buffer);

        // Even small buffers get minimum 64 byte escape reserve, but that's larger than buffer
        let stats = db.stats();
        assert_eq!(stats.escape_reserve, 64); // minimum

        // Since escape_reserve (64) > buffer.len() (10), no bytes should be appendable
        // This should not panic with underflow, but return BufferFull error
        let result = db.append_unescaped_byte(b'A');
        assert_eq!(result.unwrap_err(), DirectBufferError::BufferFull);

        // Test with even smaller buffer to ensure we handle underflow correctly
        let mut tiny_buffer = [0u8; 3];
        let mut tiny_db = DirectBuffer::new(&mut tiny_buffer);
        let tiny_stats = tiny_db.stats();
        assert_eq!(tiny_stats.escape_reserve, 64); // Still minimum 64

        // Should handle this gracefully without panic
        let result = tiny_db.append_unescaped_byte(b'B');
        assert_eq!(result.unwrap_err(), DirectBufferError::BufferFull);
    }
}

impl<'b> crate::number_parser::NumberExtractor for DirectBuffer<'b> {
    fn get_number_slice(
        &self,
        start: usize,
        end: usize,
    ) -> Result<&[u8], crate::shared::ParseError> {
        self.get_string_slice(start, end)
            .map_err(|_| crate::shared::ParseError::UnexpectedState("Invalid number slice bounds"))
    }

    fn current_position(&self) -> usize {
        self.tokenize_pos
    }

    fn is_empty(&self) -> bool {
        self.tokenize_pos >= self.data_end
    }
}
