// SPDX-License-Identifier: Apache-2.0

use crate::ParseError;
/// Shared components for JSON parsers
use crate::{ujson, JsonNumber, String};

/// Content type identification for ContentSpan events
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ContentKind {
    String,
    Key,
    Number,
}

/// Events produced by JSON parsers
#[derive(Debug, PartialEq)]
pub enum Event<'a, 'b> {
    /// The start of an object (e.g., `{`).
    StartObject,
    /// The end of an object (e.g., `}`).
    EndObject,
    /// The start of an array (e.g., `[`).
    StartArray,
    /// The end of an array (e.g., `]`).
    EndArray,
    /// An object key (e.g., `"key":`).
    Key(String<'a, 'b>),
    /// A string value (e.g., `"value"`).
    String(String<'a, 'b>),
    /// A number value (e.g., `42` or `3.14`).
    Number(JsonNumber<'a, 'b>),
    /// A boolean value (e.g., `true` or `false`).
    Bool(bool),
    /// A null value (e.g., `null`).
    Null,
    
    /// Describes a complete token found within the current input chunk.
    ContentSpan {
        /// The type of content (string, key, or number)
        kind: ContentKind,
        /// Absolute start position in the overall stream
        start: usize,
        /// Absolute end position in the overall stream
        end: usize,
        /// Whether the content contains escape sequences
        has_escapes: bool,
    },

    /// Describes the beginning of a token that is cut off by a chunk boundary.
    PartialContentSpanStart {
        /// The type of content (string, key, or number)
        kind: ContentKind,
        /// Absolute start position in the overall stream
        start: usize,
        /// Whether escapes were seen before the chunk ended
        has_escapes_in_this_chunk: bool,
    },

    /// Describes the end of a token that began in a previous chunk.
    PartialContentSpanEnd {
        /// The type of content (string, key, or number)
        kind: ContentKind,
        /// Absolute end position in the overall stream
        end: usize,
        /// Whether escapes were seen in this final chunk
        has_escapes_in_this_chunk: bool,
    },
    
    /// End of the document.
    EndDocument,
}

/// Specific unexpected states that can occur during parsing.
#[derive(Debug, PartialEq)]
pub enum UnexpectedState {
    /// A generic state mismatch occurred.
    StateMismatch,
    /// An invalid escape token was encountered.
    InvalidEscapeToken,
    /// A Unicode escape sequence was invalid.
    InvalidUnicodeEscape,
    /// An operation exceeded the buffer's capacity.
    BufferCapacityExceeded,
    /// Invalid slice bounds were provided for an operation.
    InvalidSliceBounds,
}

/// Internal parser state tracking
#[derive(Debug, PartialEq)]
pub enum State {
    None,
    Key(usize),
    String(usize),
    Number(usize),
}

/// Parser state and event storage
pub struct ParserState {
    pub evts: [Option<ujson::Event>; 2],
}

impl ParserState {
    pub fn new() -> Self {
        Self {
            evts: core::array::from_fn(|_| None),
        }
    }
}

impl Default for ParserState {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for parsers that can be used in a pull-based manner.
///
/// This trait is implemented by both `SliceParser` and `StreamParser`.
pub trait PullParser {
    /// Iterator-like method that returns None when parsing is complete.
    /// This method returns None when EndDocument is reached, Some(Ok(event)) for successful events,
    /// and Some(Err(error)) for parsing errors.
    fn next(&mut self) -> Option<Result<Event<'_, '_>, ParseError>> {
        match self.next_event() {
            Ok(Event::EndDocument) => None,
            other => Some(other),
        }
    }
    /// Returns the next JSON event or an error if parsing fails.
    /// Parsing continues until `EndDocument` is returned or an error occurs.
    fn next_event(&mut self) -> Result<Event<'_, '_>, ParseError>;
}

/// Utility for calculating common content range boundaries in JSON parsing.
/// Provides consistent position arithmetic for string/number content extraction.
pub(crate) struct ContentRange;

impl ContentRange {
    /// Calculate number content start from current position
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position (typically after first digit was processed)
    ///
    /// # Returns
    /// Position that includes the first digit of the number
    pub fn number_start_from_current(current_pos: usize) -> usize {
        current_pos.saturating_sub(1) // Back up to include first digit
    }

    /// Calculate string content boundaries using content start position
    /// Alternative to string_content_bounds that works with content positions
    ///
    /// # Arguments
    /// * `content_start` - Position where content begins (after opening quote)
    /// * `current_pos` - Current parser position (typically after closing quote)
    ///
    /// # Returns
    /// (content_start, content_end) where both positions bound the actual content
    pub fn string_content_bounds_from_content_start(
        content_start: usize,
        current_pos: usize,
    ) -> (usize, usize) {
        let content_end = current_pos.saturating_sub(1); // Back up to exclude closing quote
        if content_start > content_end {
            (content_start, content_start)
        } else {
            (content_start, content_end)
        }
    }

    /// Calculate Unicode escape sequence boundaries
    ///
    /// # Arguments
    /// * `current_pos` - Current position (after 4 hex digits)
    ///
    /// # Returns
    /// (hex_start, hex_end, escape_start) where hex_start/hex_end bound the XXXX
    /// and escape_start is the position of the backslash in \uXXXX
    pub fn unicode_escape_bounds(current_pos: usize) -> (usize, usize, usize) {
        let hex_start = current_pos.saturating_sub(4); // Start of XXXX
        let hex_end = current_pos; // End of XXXX
        let escape_start = current_pos.saturating_sub(6); // Start of \uXXXX
        (hex_start, hex_end, escape_start)
    }

    /// Calculate end position for string content
    /// Used when the parser position needs to exclude the delimiter
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position
    ///
    /// # Returns
    /// Position excluding the final delimiter
    pub fn end_position_excluding_delimiter(current_pos: usize) -> usize {
        current_pos.saturating_sub(1)
    }

    /// Calculate number end position with delimiter handling
    /// Standardizes the pattern of excluding delimiters unless at document end
    ///
    /// # Arguments
    /// * `current_pos` - Current parser position
    /// * `use_full_span` - True if the number is at the end of the document and not in a container,
    ///   meaning there is no delimiter to exclude.
    ///
    /// # Returns
    /// End position for number content
    pub fn number_end_position(current_pos: usize, use_full_span: bool) -> usize {
        if use_full_span {
            // At document end and standalone - use full span (no delimiter to exclude)
            current_pos
        } else {
            // Normal case - exclude delimiter
            current_pos.saturating_sub(1)
        }
    }
}

/// A trait that abstracts the source of JSON data for content extraction.
///
/// This trait provides a unified interface for accessing both borrowed content from
/// the original input data and unescaped content from temporary scratch buffers.
/// It enables consistent content extraction patterns across different parser types.
///
/// # Generic Parameters
///
/// * `'input` - Lifetime for the input data being parsed
/// * `'scratch` - Lifetime for the scratch buffer used for temporary storage
pub trait DataSource<'input, 'scratch> {
    /// Returns the next byte from the input source.
    /// Returns None when end of input is reached
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError>;

    /// Returns a slice of the raw, unprocessed input data from a specific range.
    /// Used for zero-copy extraction of content that contains no escape sequences.
    ///
    /// # Arguments
    /// * `start` - Start position in the input data
    /// * `end` - End position in the input data (exclusive)
    ///
    /// # Returns
    /// A slice of the input data with lifetime `'input`
    fn get_borrowed_slice(
        &'input self,
        start: usize,
        end: usize,
    ) -> Result<&'input [u8], ParseError>;

    /// Returns the full slice of the processed, unescaped content from the scratch buffer.
    /// Used when escape sequences have been processed and content written to temporary buffer.
    ///
    /// # Returns
    /// A slice of unescaped content with lifetime `'scratch`
    fn get_unescaped_slice(&'scratch self) -> Result<&'scratch [u8], ParseError>;

    /// Check if unescaped content is available in the scratch buffer.
    ///
    /// # Returns
    /// `true` if unescaped content exists and should be accessed via `get_unescaped_slice()`,
    /// `false` if content should be accessed via `get_borrowed_slice()`
    fn has_unescaped_content(&self) -> bool;
}

/// Raw content piece from either input buffer or scratch buffer.
/// This enum cleanly separates the two different content sources without
/// coupling the DataSource trait to high-level JSON types.
#[derive(Debug, PartialEq)]
pub enum ContentPiece<'input, 'scratch> {
    /// Content borrowed directly from the input buffer (zero-copy)
    Input(&'input [u8]),
    /// Content processed and stored in the scratch buffer (unescaped)
    Scratch(&'scratch [u8]),
}

impl<'input, 'scratch> ContentPiece<'input, 'scratch>
where
    'input: 'scratch,
{
    /// Convert the content piece to a String enum
    pub fn into_string(self) -> Result<String<'input, 'scratch>, ParseError> {
        match self {
            ContentPiece::Input(bytes) => {
                let content_str = from_utf8(bytes)?;
                Ok(String::Borrowed(content_str))
            }
            ContentPiece::Scratch(bytes) => {
                let content_str = from_utf8(bytes)?;
                Ok(String::Unescaped(content_str))
            }
        }
    }

    /// Returns the underlying byte slice, whether from input or scratch.
    pub fn as_bytes(&self) -> &'scratch [u8] {
        match self {
            ContentPiece::Input(bytes) => bytes,
            ContentPiece::Scratch(bytes) => bytes,
        }
    }
}

pub fn from_utf8(v: &[u8]) -> Result<&str, ParseError> {
    core::str::from_utf8(v).map_err(Into::into)
}

/// A generic helper function that uses the DataSource trait to extract the correct
/// content piece (either borrowed or from scratch). This consolidates the core
/// extraction logic for all parsers.
pub fn get_content_piece<'input, 'scratch, D>(
    source: &'input D,
    start_pos: usize,
    current_pos: usize,
) -> Result<ContentPiece<'input, 'scratch>, ParseError>
where
    'input: 'scratch,
    D: ?Sized + DataSource<'input, 'scratch>,
{
    if source.has_unescaped_content() {
        source.get_unescaped_slice().map(ContentPiece::Scratch)
    } else {
        let (content_start, content_end) =
            ContentRange::string_content_bounds_from_content_start(start_pos, current_pos);
        source
            .get_borrowed_slice(content_start, content_end)
            .map(ContentPiece::Input)
    }
}
