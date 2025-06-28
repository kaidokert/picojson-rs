/// Error type for SliceInputBuffer operations.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// Reached the end of input data.
    ReachedEnd,
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

impl<'a> InputBuffer for SliceInputBuffer<'a> {
    fn is_past_end(&self) -> bool {
        self.pos > self.data.len()
    }
    fn consume_byte(&mut self) -> Result<u8, Error> {
        if self.pos >= self.data.len() {
            self.pos += 1; // Still increment position like original logic
            return Err(Error::ReachedEnd);
        }
        let byte = self.data[self.pos];
        self.pos += 1;
        Ok(byte)
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

    /// Gets a slice of the data from start to end positions.
    pub fn slice(&self, start: usize, end: usize) -> &'a [u8] {
        &self.data[start..end]
    }

    /// Gets a slice from start position to current position - 1.
    /// Useful for extracting tokens that end at the current position.
    pub fn slice_to_current(&self, start: usize) -> &'a [u8] {
        &self.data[start..self.pos.saturating_sub(1)]
    }
}

impl<'a> crate::number_parser::NumberExtractor for SliceInputBuffer<'a> {
    fn get_number_slice(
        &self,
        start: usize,
        end: usize,
    ) -> Result<&[u8], crate::shared::ParseError> {
        if end > self.data.len() {
            return Err(crate::shared::ParseError::UnexpectedState(
                "End position beyond buffer",
            ));
        }
        Ok(&self.data[start..end])
    }

    fn current_position(&self) -> usize {
        // FlexParser's position is AFTER the delimiter that ended the number
        // We need to return the position BEFORE that delimiter for consistent behavior
        self.pos.saturating_sub(1)
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

    #[test]
    fn test_empty_buffer_boundary() {
        let data = b""; // 0 bytes
        let mut buffer = SliceInputBuffer::new(data);

        // Position 0: immediately at end for empty buffer
        assert_eq!(buffer.current_pos(), 0);
        assert_eq!(
            buffer.current_pos(),
            data.len(),
            "pos should equal data.len() for empty buffer"
        );
        assert!(
            buffer.is_past_end(),
            "Empty buffer should be past end immediately"
        );
        assert!(
            buffer.consume_byte().is_err(),
            "consume_byte() should fail on empty buffer"
        );
    }

    #[test]
    fn test_single_byte_buffer_boundary() {
        let data = b"x"; // 1 byte
        let mut buffer = SliceInputBuffer::new(data);

        // Position 0: should have data
        assert!(
            !buffer.is_past_end(),
            "Single byte buffer should not start past end"
        );
        assert_eq!(buffer.consume_byte(), Ok(b'x'));

        // Position 1: exactly at end (pos == data.len())
        assert_eq!(buffer.current_pos(), 1);
        assert_eq!(
            buffer.current_pos(),
            data.len(),
            "pos should equal data.len()"
        );
        assert!(buffer.is_past_end(), "pos == data.len() should be past end");
        assert!(
            buffer.consume_byte().is_err(),
            "consume_byte() should fail at end"
        );
    }
}
