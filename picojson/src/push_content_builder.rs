// SPDX-License-Identifier: Apache-2.0

//! Content extractor for PushParser.
//!
//! Content capture is span-based: the batched drive in `push_parser.rs`
//! feeds whole chunks to the tokenizer and only the *boundaries* of content
//! (token begin, escape begin/end, token end, chunk end) do any work here.
//! Clean runs of content bytes are never visited individually — they are
//! either borrowed straight from the input at extraction, or copied into
//! the scratch buffer as one slice per boundary.

use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::ContentExtractor;
use crate::shared::{DataSource, State};
use crate::stream_buffer::StreamBuffer;
use crate::{Event, JsonNumber, ParseError};

/// A trait for handling events from a SAX-style push parser.
///
/// # Generic Parameters
///
/// * `'input` - Lifetime for the input data being parsed
/// * `'scratch` - Lifetime for the scratch buffer used for temporary storage
/// * `E` - The error type that can be returned by the handler
pub trait PushParserHandler<'input, 'scratch, E> {
    /// Handles a single, complete JSON event.
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), E>;
}

/// Sentinel for `seg_start` while inside an escape sequence: the bytes of
/// `\n`-style or `\uXXXX` escapes are never content, so there is no pending
/// clean segment until the escape completes and restarts one.
const SEG_IN_ESCAPE: usize = usize::MAX;

/// Persistent content-extraction state for PushParser.
///
/// Holds everything that must survive across `write()` calls. The input chunk
/// itself deliberately does *not* live here: it is passed to
/// [`PushChunkExtractor`] per call, so callers may feed chunks from a reused
/// buffer (each chunk's borrow ends when `write()` returns).
pub struct PushContentBuilder<'scratch> {
    /// StreamBuffer for single-buffer input and escape processing
    stream_buffer: StreamBuffer<'scratch>,
    /// Parser state tracking
    parser_state: State,
    /// Unicode escape collector for \uXXXX sequences
    unicode_escape_collector: UnicodeEscapeCollector,
    /// Flag to reset unescaped content on next operation
    unescaped_reset_queued: bool,
    /// Position offset for tracking absolute positions across chunks
    position_offset: usize,
    /// Absolute position of the byte that produced the event being processed
    current_position: usize,
    /// Whether the current token's content lives in the scratch buffer
    /// (forced by an escape or by a chunk boundary mid-token)
    scratch_mode: bool,
    /// Absolute position of the first content byte not yet copied to
    /// scratch, or [`SEG_IN_ESCAPE`] while inside an escape sequence.
    /// Only meaningful while `parser_state` is a token state.
    seg_start: usize,
    /// Absolute position of the first hex digit of an in-flight `\uXXXX`
    /// escape (the tokenizer emits `Begin(UnicodeEscape)` at that digit).
    unicode_hex_start: Option<usize>,
}

impl<'scratch> PushContentBuilder<'scratch> {
    /// Create a new PushContentBuilder
    pub fn new(buffer: &'scratch mut [u8]) -> Self {
        Self {
            stream_buffer: StreamBuffer::new(buffer),
            parser_state: State::None,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            unescaped_reset_queued: false,
            position_offset: 0,
            current_position: 0,
            scratch_mode: false,
            seg_start: 0,
            unicode_hex_start: None,
        }
    }

    /// Current parser state (used by `PushParser::finish` after the last chunk).
    pub(crate) fn parser_state(&self) -> &State {
        &self.parser_state
    }

    /// Update position offset by adding to it
    pub fn add_position_offset(&mut self, amount: usize) {
        self.position_offset += amount;
    }

    /// Apply queued unescaped content reset if needed
    pub fn apply_unescaped_reset_if_queued(&mut self) {
        if self.unescaped_reset_queued {
            self.unescaped_reset_queued = false;
            self.scratch_mode = false;
            self.stream_buffer.clear_unescaped();
        }
    }

    /// Queue a reset of unescaped content for the next operation
    fn queue_unescaped_reset(&mut self) {
        self.unescaped_reset_queued = true;
    }

    /// Whether the current token's content lives in the scratch buffer.
    fn has_unescaped_content(&self) -> bool {
        self.scratch_mode
    }
}

/// Per-call extractor for `PushParser::write`.
///
/// Pairs the persistent [`PushContentBuilder`] with the chunk passed to the
/// current `write()` call. Exists so the chunk's lifetime is scoped to the
/// call rather than to the parser: any partial token is copied into the
/// scratch buffer before `write()` returns, so nothing borrowed from the
/// chunk survives this struct.
pub(crate) struct PushChunkExtractor<'a, 'chunk, 'scratch> {
    builder: &'a mut PushContentBuilder<'scratch>,
    chunk: &'chunk [u8],
}

impl<'a, 'chunk, 'scratch> PushChunkExtractor<'a, 'chunk, 'scratch> {
    /// Bind `builder` to the chunk being processed by this `write()` call.
    pub(crate) fn new(builder: &'a mut PushContentBuilder<'scratch>, chunk: &'chunk [u8]) -> Self {
        Self { builder, chunk }
    }

    /// Record the absolute position of the byte that produced the event the
    /// drive is about to dispatch. All boundary arithmetic keys off this.
    pub(crate) fn set_event_position(&mut self, rel_pos: usize) {
        self.builder.current_position = self.builder.position_offset + rel_pos;
    }

    /// See [`PushContentBuilder::apply_unescaped_reset_if_queued`].
    pub(crate) fn apply_unescaped_reset_if_queued(&mut self) {
        self.builder.apply_unescaped_reset_if_queued();
    }

    /// Begin span bookkeeping for a number token whose first byte sits at
    /// `first_byte_pos` (numbers never clear the scratch buffer on begin —
    /// the post-event reset already did — and never contain escapes).
    pub(crate) fn begin_number_span(&mut self, first_byte_pos: usize) {
        self.builder.scratch_mode = false;
        self.builder.seg_start = first_byte_pos;
        self.builder.unicode_hex_start = None;
    }

    /// The chunk slice for an absolute position range, bounds-checked.
    fn chunk_span(&self, start_abs: usize, end_abs: usize) -> Result<&'chunk [u8], ParseError> {
        let lo = start_abs.saturating_sub(self.builder.position_offset);
        let hi = end_abs.saturating_sub(self.builder.position_offset);
        if hi > self.chunk.len() || lo > hi {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }
        Ok(&self.chunk[lo..hi])
    }

    /// Copy the pending clean segment `[seg_start, upto_abs)` to scratch as
    /// one slice. No-op inside escapes or when the segment is empty.
    fn append_pending(&mut self, upto_abs: usize) -> Result<(), ParseError> {
        let seg = self.builder.seg_start;
        if seg != SEG_IN_ESCAPE && seg < upto_abs {
            let span = self.chunk_span(seg, upto_abs)?;
            self.builder
                .stream_buffer
                .append_unescaped_slice(span)
                .map_err(ParseError::from)?;
            self.builder.seg_start = upto_abs;
        }
        Ok(())
    }

    /// Feed in-chunk hex digits of the in-flight `\uXXXX` escape to the
    /// collector: `[from_abs, upto_abs)` clamped to this chunk.
    fn feed_hex_span(&mut self, from_abs: usize, upto_abs: usize) -> Result<(), ParseError> {
        let from = from_abs.max(self.builder.position_offset);
        let span = self.chunk_span(from, upto_abs)?;
        for &digit in span {
            self.builder.unicode_escape_collector.add_hex_digit(digit)?;
        }
        Ok(())
    }

    /// Complete an `\uXXXX` escape whose final hex digit produced the
    /// `End(UnicodeEscape)` event at `current_position`. Digits from earlier
    /// chunks were already fed by [`Self::chunk_end_flush`].
    fn complete_unicode_escape(&mut self) -> Result<(), ParseError> {
        let end_pos = self.builder.current_position;
        let Some(hex_start) = self.builder.unicode_hex_start.take() else {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ));
        };
        self.feed_hex_span(hex_start, end_pos + 1)?;

        let mut utf8_buffer = [0u8; 4];
        let (utf8_bytes_opt, _surrogate_state_changed) = self
            .builder
            .unicode_escape_collector
            .process_to_utf8(&mut utf8_buffer)?;
        if let Some(utf8_bytes) = utf8_bytes_opt {
            self.builder
                .stream_buffer
                .append_unescaped_slice(utf8_bytes)
                .map_err(ParseError::from)?;
        }
        // A pending high surrogate appends nothing yet; either way the next
        // clean segment starts after the fourth hex digit.
        self.builder.seg_start = end_pos + 1;
        Ok(())
    }

    /// Chunk-boundary handling, called once per `write()` after the whole
    /// chunk has been tokenized: any token still in progress has its
    /// in-chunk bytes preserved (content to scratch, in-flight hex digits to
    /// the collector), so the caller may reuse the chunk's storage.
    pub(crate) fn chunk_end_flush(&mut self) -> Result<(), ParseError> {
        if matches!(self.builder.parser_state, State::None) {
            return Ok(());
        }
        let chunk_end_abs = self.builder.position_offset + self.chunk.len();
        if let Some(hex_start) = self.builder.unicode_hex_start {
            // Mid-\uXXXX: the digits are collector state, not content.
            return self.feed_hex_span(hex_start, chunk_end_abs);
        }
        if self.builder.seg_start == SEG_IN_ESCAPE {
            // Between the backslash and its escape byte: nothing is content.
            return Ok(());
        }
        // Only a NONZERO pending prefix forces scratch mode. A token whose
        // content starts exactly at the next chunk's first byte (opening
        // quote as this chunk's last byte, or a fully-empty split string)
        // stays borrowable: its content bounds align with the next chunk,
        // which is the byte-wise drive's exact behavior — including the
        // Borrowed-vs-Unescaped identity of the resulting event.
        if self.builder.seg_start < chunk_end_abs {
            self.builder.scratch_mode = true;
            self.append_pending(chunk_end_abs)?;
        }
        Ok(())
    }
}

impl ContentExtractor for PushChunkExtractor<'_, '_, '_> {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError> {
        // The batched drive hands whole chunks to the tokenizer; nothing
        // pulls bytes through the extractor.
        Ok(None)
    }

    fn parser_state_mut(&mut self) -> &mut State {
        &mut self.builder.parser_state
    }

    fn parser_state(&self) -> &State {
        &self.builder.parser_state
    }

    fn unicode_escape_collector_mut(&mut self) -> &mut UnicodeEscapeCollector {
        &mut self.builder.unicode_escape_collector
    }

    fn current_position(&self) -> usize {
        self.builder.current_position
    }

    fn begin_string_content(&mut self, pos: usize) {
        self.builder.stream_buffer.clear_unescaped();
        self.builder.scratch_mode = false;
        self.builder.seg_start = pos + 1;
        self.builder.unicode_hex_start = None;
    }

    fn extract_string_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        // `current_position` is the closing quote: flush the last clean
        // segment (scratch tokens only), and queue the post-event reset.
        if self.builder.scratch_mode {
            self.append_pending(self.builder.current_position)?;
            self.builder.queue_unescaped_reset();
        }

        // PushParser: current_position points AT the closing quote, but get_content_piece expects
        // position AFTER the closing quote, so add 1
        let content_piece = crate::shared::get_content_piece(
            self,
            start_pos + 1,
            self.builder.current_position + 1,
        )?;
        content_piece.into_string().map(Event::String)
    }

    fn extract_key_content(&mut self, start_pos: usize) -> Result<Event<'_, '_>, ParseError> {
        if self.builder.scratch_mode {
            self.append_pending(self.builder.current_position)?;
            self.builder.queue_unescaped_reset();
        }

        let content_piece = crate::shared::get_content_piece(
            self,
            start_pos + 1,
            self.builder.current_position + 1,
        )?;
        content_piece.into_string().map(Event::Key)
    }

    fn extract_number(
        &mut self,
        start_pos: usize,
        _from_container_end: bool,
        _finished: bool,
    ) -> Result<Event<'_, '_>, ParseError> {
        // `current_position` is the delimiter that ended the number.
        if self.builder.scratch_mode {
            self.append_pending(self.builder.current_position)?;
            self.builder.queue_unescaped_reset();
        }

        let content_piece = crate::shared::get_content_piece(
            self,
            start_pos + 1,
            self.builder.current_position + 1,
        )?;
        let number_bytes = content_piece.as_bytes();
        let json_number = JsonNumber::from_slice(number_bytes)?;
        Ok(Event::Number(json_number))
    }

    fn begin_escape_sequence(&mut self) -> Result<(), ParseError> {
        // Copy-on-escape, span form: everything clean up to the backslash
        // moves to scratch in one slice; the escape bytes themselves are
        // never content, so the pending segment goes dormant until the
        // escape completes.
        self.append_pending(self.builder.current_position)?;
        self.builder.scratch_mode = true;
        self.builder.seg_start = SEG_IN_ESCAPE;
        Ok(())
    }

    fn begin_unicode_escape(&mut self) -> Result<(), ParseError> {
        // The tokenizer emits Begin(UnicodeEscape) at the FIRST hex digit
        // (it has already consumed `\u`), so `current_position` is where the
        // four-digit hex run starts.
        self.builder.unicode_hex_start = Some(self.builder.current_position);
        Ok(())
    }

    fn handle_simple_escape_char(&mut self, escape_char: u8) -> Result<(), ParseError> {
        if !self.builder.scratch_mode {
            // begin_escape_sequence always runs first for a well-formed
            // escape; anything else is an internal state error.
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::StateMismatch,
            ));
        }
        self.builder
            .stream_buffer
            .append_unescaped_byte(escape_char)
            .map_err(ParseError::from)?;
        // The escape byte (e.g. the `n` of `\n`) sits at current_position;
        // clean content resumes right after it.
        self.builder.seg_start = self.builder.current_position + 1;
        Ok(())
    }

    fn process_unicode_escape_with_collector(&mut self) -> Result<(), ParseError> {
        // Called at End(UnicodeEscape), whose position is the fourth hex
        // digit — the moment the escape's UTF-8 lands in scratch.
        self.complete_unicode_escape()
    }
}

// The trait lifetimes are deliberately independent of the struct's: callers
// (`get_content_piece` via the extract_* methods) borrow the extractor for a
// short reborrow `'i`, not for the full `'chunk`/`'scratch`. Tying them
// together would require a `&'chunk`-long borrow of a view that lives shorter
// than the chunk. The `'chunk: 'i` / `'scratch: 's` relations the method
// bodies need are implied bounds of the `&'i self` / `&'s self` receivers.
impl<'i, 's, 'a, 'chunk, 'scratch> DataSource<'i, 's> for PushChunkExtractor<'a, 'chunk, 'scratch> {
    fn get_borrowed_slice(&'i self, start: usize, end: usize) -> Result<&'i [u8], ParseError> {
        // Convert absolute positions to relative positions within the current chunk
        let slice_start = start.saturating_sub(self.builder.position_offset);
        let slice_end = end.saturating_sub(self.builder.position_offset);

        // Check if the requested range is within the current chunk
        if slice_end > self.chunk.len() || slice_start > slice_end {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }

        let result = &self.chunk[slice_start..slice_end];
        Ok(result)
    }

    fn get_unescaped_slice(&'s self) -> Result<&'s [u8], ParseError> {
        self.builder
            .stream_buffer
            .get_unescaped_slice()
            .map_err(ParseError::from)
    }

    fn has_unescaped_content(&self) -> bool {
        self.builder.has_unescaped_content()
    }
}
