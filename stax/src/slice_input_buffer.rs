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
