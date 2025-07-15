// SPDX-License-Identifier: Apache-2.0

//! Content building and extraction trait for unifying parser content handling.
//!
//! This module provides the `ContentBuilder` trait that consolidates content extraction
//! and escape handling logic between SliceParser and StreamParser, eliminating duplication
//! while preserving each parser's performance characteristics.

use crate::event_processor::{ContentExtractor, EscapeHandler};
use crate::{Event, ParseError, String};

/// Trait for building and extracting content (strings, keys, numbers) with escape handling.
///
/// This trait combines the functionality of `ContentExtractor` and `EscapeHandler` into a
/// unified interface that can be implemented by different parser backends while sharing
/// the core event processing logic.
#[allow(dead_code)] // Methods are part of trait interface design
pub trait ContentBuilder: ContentExtractor + EscapeHandler {
    /// Begin processing a new string or key at the given position
    ///
    /// # Arguments
    /// * `pos` - Position where the content starts
    /// * `is_key` - True if this is a key, false if it's a string value
    fn begin_content(&mut self, pos: usize, is_key: bool);

    /// Handle a simple escape character (after EscapeProcessor conversion)
    ///
    /// # Arguments
    /// * `escape_char` - The unescaped character (e.g., b'\n' for "\\n")
    fn handle_simple_escape(&mut self, escape_char: u8) -> Result<(), ParseError>;

    /// Handle a Unicode escape sequence, providing the resulting UTF-8 bytes
    ///
    /// # Arguments
    /// * `utf8_bytes` - The UTF-8 encoded bytes for the Unicode codepoint
    fn handle_unicode_escape(&mut self, utf8_bytes: &[u8]) -> Result<(), ParseError>;

    /// Append a literal (non-escape) byte during content accumulation
    ///
    /// # Arguments
    /// * `byte` - The literal byte to append
    fn append_literal_byte(&mut self, byte: u8) -> Result<(), ParseError>;

    /// Begin an escape sequence (lifecycle hook)
    /// Called when escape sequence processing begins (e.g., on Begin(EscapeSequence))
    fn begin_escape_sequence(&mut self) -> Result<(), ParseError>;

    /// Extract a completed string value
    ///
    /// # Arguments
    /// * `start_pos` - Position where the string content started
    fn extract_string(&mut self, start_pos: usize) -> Result<String<'_, '_>, ParseError>;

    /// Extract a completed key
    ///
    /// # Arguments
    /// * `start_pos` - Position where the key content started
    fn extract_key(&mut self, start_pos: usize) -> Result<String<'_, '_>, ParseError>;

    /// Extract a completed number using shared number parsing logic
    ///
    /// # Arguments
    /// * `start_pos` - Position where the number started
    /// * `from_container_end` - True if number was terminated by container delimiter
    /// * `finished` - True if the parser has finished processing input (StreamParser-specific)
    fn extract_number(
        &mut self,
        start_pos: usize,
        from_container_end: bool,
        finished: bool,
    ) -> Result<Event<'_, '_>, ParseError>;

    /// Get the current position in the input
    fn current_position(&self) -> usize;

    /// Check if input is exhausted (for number delimiter logic)
    fn is_exhausted(&self) -> bool;
}
