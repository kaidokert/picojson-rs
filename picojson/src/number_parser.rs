// SPDX-License-Identifier: Apache-2.0

use crate::parse_error::ParseError;
use crate::shared::Event;
#[cfg(test)]
use crate::shared::UnexpectedState;
use crate::JsonNumber;

/// Trait for extracting number slices from different buffer implementations.
pub trait NumberExtractor {
    /// Extract a slice of bytes representing a number from start to end position.
    ///
    /// # Arguments
    /// * `start` - The starting position of the number (inclusive)
    /// * `end` - The ending position of the number (exclusive)
    ///
    /// # Returns
    /// A byte slice containing the number content
    fn get_number_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError>;

    /// Get the current position in the buffer for end position calculation.
    fn current_position(&self) -> usize;

    /// Check if the buffer is empty (used for delimiter logic).
    fn is_empty(&self) -> bool;
}

/// Shared number parsing with automatic delimiter handling.
///
/// This function encapsulates the complete number parsing pattern:
/// 1. Calculate correct end position using delimiter logic
/// 2. Extract number slice from buffer
/// 3. Convert to UTF-8 string
/// 4. Parse using shared number parsing logic
/// 5. Create JsonNumber::Borrowed event
///
/// # Arguments
/// * `extractor` - Buffer that implements NumberExtractor
/// * `start_pos` - Starting position of the number content
/// * `from_container_end` - True if number is terminated by container delimiter
/// * `at_document_end` - True if we're at end of document (no delimiter to exclude)
pub fn parse_number_with_delimiter_logic<T: NumberExtractor>(
    extractor: &T,
    start_pos: usize,
    from_container_end: bool,
    at_document_end: bool,
) -> Result<Event<'_, '_>, ParseError> {
    let current_pos = extractor.current_position();

    // A standalone number at the end of the document has no trailing delimiter, so we use the full span.
    let use_full_span = !from_container_end && at_document_end;
    let end_pos = crate::shared::ContentRange::number_end_position(current_pos, use_full_span);

    parse_number_event(extractor, start_pos, end_pos)
}

/// Number parsing with explicit position control - used by both parsers.
///
/// This function encapsulates the common pattern:
/// 1. Extract number slice from buffer
/// 2. Convert to UTF-8 string
/// 3. Parse using shared number parsing logic
/// 4. Create JsonNumber::Borrowed event
///
/// All position logic is handled by the calling parser.
pub fn parse_number_event<T: NumberExtractor>(
    extractor: &T,
    start_pos: usize,
    end_pos: usize,
) -> Result<Event<'_, '_>, ParseError> {
    let number_bytes = extractor.get_number_slice(start_pos, end_pos)?;
    let parsed_result = crate::parse_number_from_str(number_bytes)?;
    let number_str = crate::shared::from_utf8(number_bytes)?;

    Ok(Event::Number(JsonNumber::Borrowed {
        raw: number_str,
        parsed: parsed_result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock extractor for testing
    struct MockExtractor {
        data: &'static [u8],
        position: usize,
        empty: bool,
    }

    impl MockExtractor {
        fn new(data: &'static [u8], position: usize, empty: bool) -> Self {
            Self {
                data,
                position,
                empty,
            }
        }
    }

    impl NumberExtractor for MockExtractor {
        fn get_number_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError> {
            if end > self.data.len() {
                return Err(UnexpectedState::InvalidSliceBounds.into());
            }
            Ok(&self.data[start..end])
        }

        fn current_position(&self) -> usize {
            self.position
        }

        fn is_empty(&self) -> bool {
            self.empty
        }
    }

    #[test]
    fn test_parse_number_event_with_container() {
        let data = b"56}"; // Number followed by container end
        let extractor = MockExtractor::new(data, 3, false); // Position after '}'

        let result = parse_number_event(&extractor, 0, 2).unwrap();
        if let Event::Number(num) = result {
            assert_eq!(num.as_str(), "56"); // Should exclude the '}'
            assert_eq!(num.as_int(), Some(56));
        } else {
            panic!("Expected Number event");
        }
    }

    #[test]
    fn test_parse_number_event_at_eof() {
        let data = b"89";
        let extractor = MockExtractor::new(data, 2, true); // At EOF

        let result = parse_number_event(&extractor, 0, 2).unwrap();
        if let Event::Number(num) = result {
            assert_eq!(num.as_str(), "89"); // Should include full number
            assert_eq!(num.as_int(), Some(89));
        } else {
            panic!("Expected Number event");
        }
    }
}
