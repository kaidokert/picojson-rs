// SPDX-License-Identifier: Apache-2.0

use crate::ParseError;

/// Error type for SliceInputBuffer operations.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// Reached the end of input data.
    ReachedEnd,
    /// Invalid slice bounds provided.
    InvalidSliceBounds,
}

/// A buffer that manages input data and current parsing position.
/// This encapsulates the data slice and position that are always used together.
#[derive(Debug)]
pub struct SliceInputBuffer<'a> {
    data: &'a [u8],
    pos: usize,
}

pub trait InputBuffer {
    fn is_past_end(&self) -> bool;
    fn consume_byte(&mut self) -> Result<u8, Error>;
}

impl InputBuffer for SliceInputBuffer<'_> {
    fn is_past_end(&self) -> bool {
        self.pos > self.data.len()
    }
    fn consume_byte(&mut self) -> Result<u8, Error> {
        match self.data.get(self.pos) {
            Some(&byte) => {
                self.pos = self.pos.checked_add(1).ok_or(Error::InvalidSliceBounds)?;
                Ok(byte)
            }
            None => {
                self.pos = self.pos.checked_add(1).ok_or(Error::InvalidSliceBounds)?;
                Err(Error::ReachedEnd)
            }
        }
    }
}
impl<'a> SliceInputBuffer<'a> {
    pub fn current_pos(&self) -> usize {
        self.pos
    }
    /// Creates a new SliceInputBuffer with the given data.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Gets a slice of the data from start to end positions, with bounds checking.
    pub fn slice(&self, start: usize, end: usize) -> Result<&'a [u8], Error> {
        self.data.get(start..end).ok_or(Error::InvalidSliceBounds)
    }

    /// Gets the length of the underlying data for bounds checking.
    pub fn data_len(&self) -> usize {
        self.data.len()
    }
}

impl crate::number_parser::NumberExtractor for SliceInputBuffer<'_> {
    fn get_number_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError> {
        self.slice(start, end)
            .map_err(|_| ParseError::InvalidNumber)
    }

    fn current_position(&self) -> usize {
        // Return the actual current position (AFTER any delimiter)
        // Delimiter handling is now centralized in parse_number_event()
        self.pos
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_boundary_behavior() {
        let data = b"abc"; // 3 bytes: positions 0, 1, 2 are valid
        let mut buffer = SliceInputBuffer::new(data);

        // Position 0: start, should have data
        assert_eq!(buffer.current_pos(), 0);
        assert!(!buffer.is_past_end(), "pos=0 should not be past end");
        assert_eq!(buffer.consume_byte(), Ok(b'a'));

        // Position 1: middle, should have data
        assert_eq!(buffer.current_pos(), 1);
        assert!(!buffer.is_past_end(), "pos=1 should not be past end");
        assert_eq!(buffer.consume_byte(), Ok(b'b'));

        // Position 2: last byte, should have data
        assert_eq!(buffer.current_pos(), 2);
        assert!(!buffer.is_past_end(), "pos=2 should not be past end");
        assert_eq!(buffer.consume_byte(), Ok(b'c'));

        // Position 3: exactly at end (pos == data.len()), no more data
        assert_eq!(buffer.current_pos(), 3);
        assert_eq!(
            buffer.current_pos(),
            data.len(),
            "pos should equal data.len()"
        );

        // INTENTIONAL DESIGN: Different semantics when pos == data.len()
        // - is_past_end() returns false (parser can still finish processing)
        // - consume_byte() returns Err (no more bytes to read)
        // This allows the tokenizer to complete final events (like EndObject)
        // even when no input bytes remain to be consumed
        assert!(
            !buffer.is_past_end(),
            "pos == data.len() should NOT be past end (allows tokenizer.finish())"
        );
        assert!(
            buffer.consume_byte().is_err(),
            "consume_byte() should fail when pos == data.len() (no bytes)"
        );

        // Position 4: past end (pos > data.len()), definitely error
        assert_eq!(buffer.current_pos(), 4);
        assert!(buffer.is_past_end(), "pos > data.len() should be past end");
        assert!(
            buffer.consume_byte().is_err(),
            "consume_byte() should fail when pos > data.len()"
        );
    }
}
