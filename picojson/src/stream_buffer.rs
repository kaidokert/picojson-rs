// SPDX-License-Identifier: Apache-2.0

/// Error types for StreamBuffer operations
#[derive(Debug, PartialEq)]
pub enum StreamBufferError {
    /// Buffer is full and cannot accommodate more data
    BufferFull,
    /// Attempted to read beyond available data
    EndOfData,
    /// An unexpected error occurred.
    Unexpected,
    /// Invalid slice bounds provided for string extraction
    InvalidSliceBounds,
}

/// StreamBuffer manages a single buffer for both input and escape processing
///
/// Key design principles:
/// - Reader fills unused portions of buffer directly
/// - Unescaped content is copied to buffer start when needed
/// - Zero-copy string extraction when no escapes are present
/// - Guaranteed space for escape processing (unescaped ≤ escaped)
pub struct StreamBuffer<'a> {
    /// The entire buffer slice
    buffer: &'a mut [u8],
    /// Current position where tokenizer is reading
    tokenize_pos: usize,
    /// End of valid data from Reader (buffer[0..data_end] contains valid data)
    data_end: usize,
    /// Length of unescaped content at buffer start (0 if no unescaping active)
    unescaped_len: usize,
}

impl<'a> StreamBuffer<'a> {
    /// Panic-free copy_within implementation that handles overlapping ranges
    /// Based on memmove behavior but without panic machinery
    fn safe_copy_within(&mut self, src_start: usize, src_end: usize, dest: usize) {
        let count = src_end.saturating_sub(src_start);

        // Early return if nothing to copy or bounds are invalid
        if count == 0
            || src_start >= self.buffer.len()
            || src_end > self.buffer.len()
            || dest >= self.buffer.len()
        {
            return;
        }

        // Ensure dest + count doesn't exceed buffer
        let max_copy = (self.buffer.len().saturating_sub(dest)).min(count);
        if max_copy == 0 {
            return;
        }

        let iterator: &mut dyn Iterator<Item = usize> = if dest <= src_start {
            &mut (0..max_copy)
        } else {
            &mut (0..max_copy).rev()
        };

        for i in iterator {
            if let (Some(src_byte), Some(dest_slot)) = (
                self.buffer.get(src_start.wrapping_add(i)).copied(),
                self.buffer.get_mut(dest.wrapping_add(i)),
            ) {
                *dest_slot = src_byte;
            }
        }
    }
    /// Create a new StreamBuffer with the given buffer slice
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            tokenize_pos: 0,
            data_end: 0,
            unescaped_len: 0,
        }
    }

    /// Get the current byte at tokenize position
    pub fn current_byte(&self) -> Result<u8, StreamBufferError> {
        if self.tokenize_pos >= self.data_end {
            return Err(StreamBufferError::EndOfData);
        }
        self.buffer
            .get(self.tokenize_pos)
            .copied()
            .ok_or(StreamBufferError::EndOfData)
    }

    /// Advance the tokenize position by one byte
    pub fn advance(&mut self) -> Result<(), StreamBufferError> {
        if self.tokenize_pos >= self.data_end {
            return Err(StreamBufferError::EndOfData);
        }
        self.tokenize_pos = self.tokenize_pos.wrapping_add(1);
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
        self.buffer.get_mut(self.data_end..)
    }

    /// Compact buffer by moving unprocessed data from a given start offset to the beginning.
    ///
    /// # Arguments
    /// * `start_offset` - The position from which to preserve data.
    ///
    /// Returns the offset by which data was moved.
    pub fn compact_from(&mut self, start_offset: usize) -> Result<usize, StreamBufferError> {
        if start_offset == 0 {
            // Already at start, no compaction possible
            return Ok(0);
        }

        let offset = start_offset;

        if start_offset >= self.data_end {
            // All data has been processed, reset to start
            self.tokenize_pos = 0;
            self.data_end = 0;
            return Ok(offset);
        }

        // Move unprocessed data to start of buffer
        let remaining_data = self.data_end.saturating_sub(start_offset);

        // Copy existing content if there is any - EXACT same pattern as start_unescaping_with_copy
        if self.data_end > start_offset {
            let span_len = remaining_data;

            // Ensure the span fits in the buffer - return error instead of silent truncation
            if span_len > self.buffer.len() {
                return Err(StreamBufferError::BufferFull);
            }

            let src_range_end = start_offset
                .checked_add(span_len)
                .ok_or(StreamBufferError::InvalidSliceBounds)?;

            if src_range_end > self.buffer.len() {
                return Err(StreamBufferError::InvalidSliceBounds);
            }
            let src_range = start_offset..src_range_end;

            // Copy within the same buffer: move data from [start_offset..end] to [0..span_len]
            // Use our panic-free copy implementation
            self.safe_copy_within(src_range.start, src_range.end, 0);
        }

        // Update positions
        self.tokenize_pos = self.tokenize_pos.saturating_sub(offset);
        self.data_end = remaining_data;

        Ok(offset)
    }

    /// Mark that Reader filled `bytes_read` bytes
    pub fn mark_filled(&mut self, bytes_read: usize) -> Result<(), StreamBufferError> {
        let new_data_end = self.data_end.wrapping_add(bytes_read);
        if new_data_end > self.buffer.len() {
            return Err(StreamBufferError::Unexpected);
        }
        self.data_end = new_data_end;
        Ok(())
    }

    /// Start unescaping and copy existing content from a range in the buffer
    /// This handles the common case of starting escape processing partway through a string
    pub fn start_unescaping_with_copy(
        &mut self,
        max_escaped_len: usize,
        copy_start: usize,
        copy_end: usize,
    ) -> Result<(), StreamBufferError> {
        // Clear any previous unescaped content
        self.unescaped_len = 0;

        // Ensure we have space at the start for unescaping
        if max_escaped_len > self.buffer.len() {
            return Err(StreamBufferError::BufferFull);
        }

        // Copy existing content if there is any
        if copy_end > copy_start && copy_start < self.data_end {
            let span_len = copy_end.saturating_sub(copy_start);

            // Ensure the span fits in the buffer - return error instead of silent truncation
            if span_len > self.buffer.len() {
                return Err(StreamBufferError::BufferFull);
            }

            let src_range = copy_start..copy_start.wrapping_add(span_len);
            if src_range.end > self.buffer.len() {
                return Err(StreamBufferError::InvalidSliceBounds);
            }

            // Copy within the same buffer: move data from [copy_start..copy_end] to [0..span_len]
            // Use our panic-free copy implementation to handle overlapping ranges safely
            self.safe_copy_within(src_range.start, src_range.end, 0);
            self.unescaped_len = span_len;
        }

        Ok(())
    }

    /// Get the unescaped content slice
    pub fn get_unescaped_slice(&self) -> Result<&[u8], StreamBufferError> {
        if self.unescaped_len == 0 {
            return Err(StreamBufferError::InvalidSliceBounds);
        }
        self.buffer
            .get(0..self.unescaped_len)
            .ok_or(StreamBufferError::Unexpected)
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
    pub fn append_unescaped_byte(&mut self, byte: u8) -> Result<(), StreamBufferError> {
        if let Some(b) = self.buffer.get_mut(self.unescaped_len) {
            *b = byte;
            self.unescaped_len = self.unescaped_len.wrapping_add(1);
            Ok(())
        } else {
            Err(StreamBufferError::BufferFull)
        }
    }

    /// Truncate unescaped content by removing the specified number of bytes from the end
    pub fn truncate_unescaped_by(&mut self, count: usize) {
        self.unescaped_len = self.unescaped_len.saturating_sub(count);
    }

    /// Get a string slice from the buffer (zero-copy)
    /// Used for strings without escapes
    pub fn get_string_slice(&self, start: usize, end: usize) -> Result<&[u8], StreamBufferError> {
        if start > end || end > self.data_end {
            return Err(StreamBufferError::InvalidSliceBounds);
        }
        self.buffer
            .get(start..end)
            .ok_or(StreamBufferError::InvalidSliceBounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::State;

    #[test]
    fn test_lifetime_expectations() {
        // This test demonstrates how StreamBuffer lifetimes should work
        let mut buffer = [0u8; 100];
        let mut stream_buffer = StreamBuffer::new(&mut buffer);

        // Simulate some data being in the buffer
        let test_data = b"hello world";
        stream_buffer.buffer[0..test_data.len()].copy_from_slice(test_data);
        stream_buffer.data_end = test_data.len();

        // Test that we can get buffer data

        // Test unescaped content - add some unescaped data
        stream_buffer.unescaped_len = 3;
        stream_buffer.buffer[0..3].copy_from_slice(b"abc");

        let unescaped_slice = stream_buffer.get_unescaped_slice().unwrap();
        assert_eq!(unescaped_slice, b"abc");

        // The key expectation: these slices should live as long as the original buffer
        // and be usable to create String::Borrowed(&'buffer str) and String::Unescaped(&'buffer str)
    }

    #[test]
    fn test_new_stream_buffer() {
        let mut buffer = [0u8; 100];
        let db = StreamBuffer::new(&mut buffer);

        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 0);
        assert_eq!(db.unescaped_len, 0);
        assert!(db.is_empty());
    }

    #[test]
    fn test_fill_and_advance() {
        let mut buffer = [0u8; 100];
        let mut db = StreamBuffer::new(&mut buffer);

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
        let mut db = StreamBuffer::new(&mut buffer);

        // EndOfData errors
        assert_eq!(db.current_byte().unwrap_err(), StreamBufferError::EndOfData);
        assert_eq!(db.advance().unwrap_err(), StreamBufferError::EndOfData);

        // No unescaped content
        assert!(db.get_unescaped_slice().is_err());
    }

    #[test]
    fn test_buffer_full_scenario() {
        // Test what happens when buffer gets completely full
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

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
        let mut db = StreamBuffer::new(&mut buffer);

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
        let mut db = StreamBuffer::new(&mut buffer);

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
    fn test_boundary_conditions() {
        let mut buffer = [0u8; 3]; // Absolute minimum
        let mut db = StreamBuffer::new(&mut buffer);

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
        let mut db = StreamBuffer::new(&mut buffer);

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
        assert_eq!(result.unwrap_err(), StreamBufferError::BufferFull);

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
    fn test_append_unescaped_byte_uses_full_buffer() {
        let mut buffer = [0u8; 10]; // 10 byte buffer
        let mut db = StreamBuffer::new(&mut buffer);

        // Should be able to append up to buffer_len bytes (no more escape reserve!)
        for i in 0..10 {
            let result = db.append_unescaped_byte(b'A');
            assert!(result.is_ok(), "Failed at byte {}", i);
        }

        assert_eq!(db.unescaped_len, 10);

        // One more byte should fail because buffer is full
        let result = db.append_unescaped_byte(b'B');
        assert_eq!(result.unwrap_err(), StreamBufferError::BufferFull);
    }

    #[test]
    fn test_compact_basic() {
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer with data: "0123456789"
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"0123456789");
        }
        db.mark_filled(10).unwrap();

        // Process some data (advance tokenize_pos to position 4)
        for _ in 0..4 {
            db.advance().unwrap();
        }

        // Before compact: tokenize_pos=4, data_end=10, remaining="456789"
        assert_eq!(db.tokenize_pos, 4);
        assert_eq!(db.data_end, 10);
        assert_eq!(db.remaining_bytes(), 6);

        // Compact the buffer
        let offset = db.compact_from(4).unwrap();
        assert_eq!(offset, 4); // Data was moved by 4 positions

        // After compact: tokenize_pos=0, data_end=6, buffer starts with "456789"
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 6);
        assert_eq!(db.remaining_bytes(), 6);

        // Verify the data was moved correctly
        assert_eq!(db.current_byte().unwrap(), b'4');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'5');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'6');
    }

    #[test]
    fn test_compact_from_preserves_number() {
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);
        db.buffer.copy_from_slice(b"0123456789");
        db.data_end = 10;
        db.tokenize_pos = 5;
        let number_start_pos = 3;

        let offset = db.compact_from(number_start_pos).unwrap();
        assert_eq!(offset, 3);
        assert_eq!(db.tokenize_pos, 2); // 5 - 3
        assert_eq!(db.data_end, 7); // 10 - 3
        assert_eq!(&db.buffer[..db.data_end], b"3456789");
    }

    #[test]
    fn test_compact_no_op_when_at_start() {
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer with data
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice[0..5].copy_from_slice(b"hello");
        }
        db.mark_filled(5).unwrap();

        // Don't advance tokenize_pos (stays at 0)
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 5);

        // Compact should be no-op
        let offset = db.compact_from(0).unwrap();
        assert_eq!(offset, 0); // No movement occurred

        // Should be unchanged
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 5);
        assert_eq!(db.current_byte().unwrap(), b'h');
    }

    #[test]
    fn test_compact_all_data_processed() {
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer with data
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice[0..5].copy_from_slice(b"hello");
        }
        db.mark_filled(5).unwrap();

        // Process all data
        for _ in 0..5 {
            db.advance().unwrap();
        }

        // All data processed
        assert_eq!(db.tokenize_pos, 5);
        assert_eq!(db.data_end, 5);
        assert!(db.is_empty());

        // Compact should reset to start
        let offset = db.compact_from(5).unwrap();
        assert_eq!(offset, 5); // All data was processed, moved by 5

        // Should be reset to empty state
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 0);
        assert!(db.is_empty());
    }

    #[test]
    fn test_compact_enables_new_data_fill() {
        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer completely
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"0123456789");
        }
        db.mark_filled(10).unwrap();

        // Process half the data
        for _ in 0..5 {
            db.advance().unwrap();
        }

        // Buffer is full, can't get fill slice
        assert!(db.get_fill_slice().is_none());

        // Compact to make space
        let offset = db.compact_from(5).unwrap();
        assert_eq!(offset, 5); // Data moved by 5 positions

        // Now should be able to get fill slice again
        let fill_slice = db.get_fill_slice().unwrap();
        assert_eq!(fill_slice.len(), 5); // 5 bytes available (10 - 5 remaining)

        // Fill with new data
        fill_slice[0..5].copy_from_slice(b"ABCDE");
        db.mark_filled(5).unwrap();

        // Verify combined data: "56789ABCDE"
        assert_eq!(db.data_end, 10);
        assert_eq!(db.current_byte().unwrap(), b'5');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'6');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'7');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'8');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'9');
        db.advance().unwrap();
        assert_eq!(db.current_byte().unwrap(), b'A');
    }

    #[test]
    fn test_compact_with_single_byte_remaining() {
        let mut buffer = [0u8; 5];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer: "abcde"
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"abcde");
        }
        db.mark_filled(5).unwrap();

        // Process almost all data (leave one byte)
        for _ in 0..4 {
            db.advance().unwrap();
        }

        // One byte remaining
        assert_eq!(db.remaining_bytes(), 1);
        assert_eq!(db.current_byte().unwrap(), b'e');

        // Compact
        let offset = db.compact_from(4).unwrap();
        assert_eq!(offset, 4); // Moved by 4 positions

        // Should have moved the last byte to start
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 1);
        assert_eq!(db.current_byte().unwrap(), b'e');
        assert_eq!(db.remaining_bytes(), 1);

        // Should have space for 4 more bytes
        let fill_slice = db.get_fill_slice().unwrap();
        assert_eq!(fill_slice.len(), 4);
    }

    #[test]
    fn test_compact_buffer_wall_scenario() {
        // Simulate hitting the buffer wall during token processing
        // This tests the "always compact when buffer full" strategy

        let mut buffer = [0u8; 10];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer completely with: `{"hello_wo` (10 bytes, fills buffer exactly)
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"{\"hello_wo");
        }
        db.mark_filled(10).unwrap();

        // Process tokens: { " h e l l o _ w o

        // Advance to simulate tokenizer processing
        for _ in 0..10 {
            db.advance().unwrap();
        }

        // Buffer is now empty, we hit the wall
        assert!(db.is_empty());
        assert!(db.get_fill_slice().is_none()); // No space to read more

        // ALWAYS compact when hitting buffer wall
        let offset = db.compact_from(10).unwrap();
        assert_eq!(offset, 10); // Moved by 10 positions (everything was processed)

        // After compaction, buffer is reset and ready for new data
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 0);

        // Now we can read more data
        {
            let fill_slice = db.get_fill_slice().unwrap();
            assert_eq!(fill_slice.len(), 10); // Full buffer available
            fill_slice[0..3].copy_from_slice(b"rld");
        }
        db.mark_filled(3).unwrap();

        // Continue processing the string continuation
        assert_eq!(db.current_byte().unwrap(), b'r');
        assert_eq!(db.remaining_bytes(), 3);
    }

    #[test]
    fn test_compact_saves_partial_token() {
        // Test case where compaction saves partial token at end of buffer
        let mut buffer = [0u8; 8];
        let mut db = StreamBuffer::new(&mut buffer);

        // Fill buffer: {"hel|lo"} where we process up to 'l' and hit wall with "lo\"}" remaining
        {
            let fill_slice = db.get_fill_slice().unwrap();
            fill_slice.copy_from_slice(b"{\"hello\"");
        }
        db.mark_filled(8).unwrap();

        // Process: { " h e l - stop here with "lo\"" remaining
        for _ in 0..5 {
            db.advance().unwrap();
        }

        // Current state: parser at position 5, with "lo\"" remaining (3 bytes)
        assert_eq!(db.current_byte().unwrap(), b'l');
        assert_eq!(db.remaining_bytes(), 3);

        // Hit buffer wall, compact
        let offset = db.compact_from(5).unwrap();
        assert_eq!(offset, 5); // Moved data by 5 positions

        // After compaction: "lo\"" is now at start of buffer
        assert_eq!(db.tokenize_pos, 0);
        assert_eq!(db.data_end, 3);
        assert_eq!(db.current_byte().unwrap(), b'l');
        assert_eq!(db.remaining_bytes(), 3);

        // We saved 3 bytes, gained 5 bytes of space
        let fill_slice = db.get_fill_slice().unwrap();
        assert_eq!(fill_slice.len(), 5);
    }

    #[test]
    fn test_position_update_after_compaction_normal_case() {
        // Test normal position updates where positions are preserved

        // Case 1: String position preserved after compaction
        let _state = State::String(10);
        let offset = 5;

        // Simulate the position update logic
        let updated_pos = if 10 < offset {
            0 // Would need escape mode
        } else {
            10 - offset // Normal position update: 10 - 5 = 5
        };

        assert_eq!(updated_pos, 5);

        // Case 2: Key position preserved after compaction
        let key_pos = 8;
        let offset = 3;

        let updated_key_pos = if key_pos < offset {
            0 // Would need escape mode
        } else {
            key_pos - offset // Normal position update: 8 - 3 = 5
        };

        assert_eq!(updated_key_pos, 5);

        // Case 3: Number position preserved after compaction
        let number_pos = 15;
        let offset = 7;

        let updated_number_pos = if number_pos < offset {
            // Numbers should not normally lose their start position
            panic!("Number position discarded - buffer too small");
        } else {
            number_pos - offset // Normal position update: 15 - 7 = 8
        };

        assert_eq!(updated_number_pos, 8);
    }

    #[test]
    fn test_position_update_after_compaction_escape_mode_case() {
        // Test position updates where original positions are discarded (need escape mode)

        // Case 1: String position discarded - needs escape mode
        let string_pos = 3;
        let offset = 7; // Offset is larger than string position

        let needs_escape_mode = string_pos < offset;
        assert!(needs_escape_mode);

        let updated_string_pos = if needs_escape_mode {
            0 // Reset for escape mode
        } else {
            string_pos - offset
        };

        assert_eq!(updated_string_pos, 0);

        // Case 2: Key position discarded - needs escape mode
        let key_pos = 2;
        let offset = 8;

        let needs_escape_mode = key_pos < offset;
        assert!(needs_escape_mode);

        let updated_key_pos = if needs_escape_mode {
            0 // Reset for escape mode
        } else {
            key_pos - offset
        };

        assert_eq!(updated_key_pos, 0);

        // Case 3: Number position discarded - should be an error
        let number_pos = 1;
        let offset = 5;

        let should_error = number_pos < offset;
        assert!(should_error); // Numbers spanning compaction boundaries should error
    }

    #[test]
    fn test_position_update_boundary_conditions() {
        // Test exact boundary conditions for position updates

        // Case 1: Position exactly equals offset
        let pos = 5;
        let offset = 5;

        let needs_escape_mode = pos < offset; // false, pos == offset
        assert!(!needs_escape_mode);

        let updated_pos = pos - offset; // 5 - 5 = 0
        assert_eq!(updated_pos, 0);

        // Case 2: Position one less than offset (boundary case)
        let pos = 4;
        let offset = 5;

        let needs_escape_mode = pos < offset; // true, pos < offset
        assert!(needs_escape_mode);

        // Case 3: Position one more than offset (boundary case)
        let pos = 6;
        let offset = 5;

        let needs_escape_mode = pos < offset; // false, pos > offset
        assert!(!needs_escape_mode);

        let updated_pos = pos - offset; // 6 - 5 = 1
        assert_eq!(updated_pos, 1);

        // Case 4: Zero offset (no compaction occurred)
        let pos = 10;
        let offset = 0;

        let needs_escape_mode = pos < offset; // false, 10 < 0
        assert!(!needs_escape_mode);

        let updated_pos = pos - offset; // 10 - 0 = 10 (unchanged)
        assert_eq!(updated_pos, 10);
    }

    #[test]
    fn test_position_update_state_transitions() {
        // Test the complete state transition logic for different parser states

        // Case 1: State::None - no position to update
        let state = State::None;
        // No position updates needed for None state
        match state {
            State::None => {
                // No action needed - test passes
            }
            _ => panic!("Expected State::None"),
        }

        // Case 2: String state position updates
        let mut string_state = State::String(12);
        let offset = 8;

        match &mut string_state {
            State::String(pos) => {
                if *pos < offset {
                    // Would need escape mode
                    *pos = 0;
                } else {
                    *pos = pos.saturating_sub(offset); // 12 - 8 = 4
                }
            }
            _ => panic!("Expected State::String"),
        }

        match string_state {
            State::String(pos) => assert_eq!(pos, 4),
            _ => panic!("Expected State::String"),
        }

        // Case 3: Key state needing escape mode
        let mut key_state = State::Key(3);
        let offset = 10;

        match &mut key_state {
            State::Key(pos) => {
                if *pos < offset {
                    // Needs escape mode
                    *pos = 0;
                } else {
                    *pos = pos.saturating_sub(offset);
                }
            }
            _ => panic!("Expected State::Key"),
        }

        match key_state {
            State::Key(pos) => assert_eq!(pos, 0), // Reset for escape mode
            _ => panic!("Expected State::Key"),
        }

        // Case 4: Number state normal update
        let mut number_state = State::Number(20);
        let offset = 6;

        match &mut number_state {
            State::Number(pos) => {
                if *pos < offset {
                    // This should not happen for numbers in normal operation
                    panic!("Number position discarded - buffer too small");
                } else {
                    *pos = pos.saturating_sub(offset); // 20 - 6 = 14
                }
            }
            _ => panic!("Expected State::Number"),
        }

        match number_state {
            State::Number(pos) => assert_eq!(pos, 14),
            _ => panic!("Expected State::Number"),
        }
    }
}
