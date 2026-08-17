// SPDX-License-Identifier: Apache-2.0

//! A SAX-style JSON push parser.
//!
//! The drive is batched: each `write()` hands the whole chunk to the
//! tokenizer in one `parse_chunk` call and dispatches the resulting events
//! (which carry chunk-relative positions) straight from the tokenizer's
//! callback. Content is captured as position-derived spans at token/escape
//! boundaries — see `push_content_builder.rs` — so clean content bytes are
//! never visited individually by parser code.

use crate::event_processor::{ContentExtractor, EventResult, ParserCore, process_simple_events};
use crate::push_content_builder::{PushChunkExtractor, PushContentBuilder, PushParserHandler};
use crate::shared::{ContentRange, State};
use crate::stream_buffer::StreamBufferError;
use crate::ujson::EventToken;
use crate::{BitStackConfig, Event, ParseError, ujson};

/// A SAX-style JSON push parser.
///
/// Generic over BitStack storage type for configurable nesting depth. Parsing
/// events are returned to the handler.
///
/// Input chunks are borrowed only for the duration of each [`write`] call —
/// any partial token is copied into the scratch buffer before `write`
/// returns — so chunks may be fed from a reused receive buffer.
///
/// [`write`]: PushParser::write
///
/// # Generic Parameters
///
/// * `'scratch` - Lifetime for the scratch buffer used for temporary storage
/// * `H` - The event handler type that implements [`PushParserHandler`]
/// * `C` - BitStack configuration type that implements [`BitStackConfig`]
pub struct PushParser<'scratch, H, C>
where
    C: BitStackConfig,
{
    /// Content extractor that handles content extraction and event emission
    extractor: PushContentBuilder<'scratch>,
    /// The handler that receives events
    handler: H,
    /// Core parser logic shared with other parsers
    core: ParserCore<C::Bucket, C::Counter>,
}

impl<'scratch, H, C> PushParser<'scratch, H, C>
where
    C: BitStackConfig,
{
    /// Creates a new `PushParser`.
    pub fn new(handler: H, buffer: &'scratch mut [u8]) -> Self {
        Self {
            extractor: PushContentBuilder::new(buffer),
            handler,
            core: ParserCore::new_chunked(),
        }
    }

    /// Processes a chunk of input data.
    ///
    /// `data` is only borrowed for the duration of the call: any token still
    /// in progress when the chunk ends is copied into the scratch buffer, so
    /// the caller is free to overwrite the chunk's storage afterwards.
    ///
    /// After an `Err`, the parser must be discarded: event delivery stops at
    /// the failing event, but the tokenizer may have consumed bytes past it.
    pub fn write<E>(&mut self, data: &[u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Apply any queued buffer resets
        self.extractor.apply_unescaped_reset_if_queued();

        let Self {
            extractor,
            handler,
            core,
        } = self;
        let mut view = PushChunkExtractor::new(&mut *extractor, data);

        // The tokenizer callback cannot return errors, so the first failure
        // is parked here and every later event in the chunk is ignored.
        let mut pending: Option<PushParseError<E>> = None;
        let tokenizer_result = core.tokenizer.parse_chunk(data, &mut |event, rel_pos| {
            if pending.is_some() {
                return;
            }
            if let Err(error) = drive_event(&mut view, handler, event, rel_pos) {
                pending = Some(error);
            }
        });

        // An event-level failure precedes any tokenizer failure at a later
        // byte, matching the strictly serial error order of the byte-wise
        // drive this replaced.
        if let Some(error) = pending {
            return Err(error);
        }
        tokenizer_result?;

        // Preserve any in-progress token's bytes before the chunk goes away.
        view.chunk_end_flush()?;

        // Update position offset for next call
        extractor.add_position_offset(data.len());

        Ok(())
    }

    /// Finishes parsing, flushes any remaining events, and returns the handler.
    /// This method consumes the parser.
    pub fn finish<E>(mut self) -> Result<H, PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Check that the JSON document is complete (all containers closed)
        // Use a no-op callback since we don't expect any more events
        let mut no_op_callback = |_event: ujson::Event, _pos: usize| {};
        let _bytes_processed = self.core.tokenizer.finish(&mut no_op_callback)?;

        // Handle any remaining content in the buffer
        if *self.extractor.parser_state() != State::None {
            return Err(crate::push_parser::PushParseError::Parse(
                ParseError::EndOfData,
            ));
        }

        // Emit EndDocument event
        self.handler
            .handle_event(Event::EndDocument)
            .map_err(PushParseError::Handler)?;

        Ok(self.handler)
    }
}

/// Dispatch one tokenizer event: position bookkeeping, state transitions,
/// span capture at boundaries, extraction, and handler delivery.
///
/// This mirrors the OnEnd-timing arm order of
/// `ParserCore::next_event_impl_with_flags`, minus the per-byte
/// accumulation that span capture replaces.
fn drive_event<H, E>(
    view: &mut PushChunkExtractor<'_, '_, '_>,
    handler: &mut H,
    event: ujson::Event,
    rel_pos: usize,
) -> Result<(), PushParseError<E>>
where
    H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    E: From<ParseError>,
{
    view.set_event_position(rel_pos);

    if let Some(result) = process_simple_events(&event) {
        match result {
            EventResult::Complete(user_event) => {
                handler
                    .handle_event(user_event)
                    .map_err(PushParseError::Handler)?;
                view.apply_unescaped_reset_if_queued();
            }
            EventResult::ExtractString => {
                let user_event = view.validate_and_extract_string()?;
                handler
                    .handle_event(user_event)
                    .map_err(PushParseError::Handler)?;
                view.apply_unescaped_reset_if_queued();
            }
            EventResult::ExtractKey => {
                let user_event = view.validate_and_extract_key()?;
                handler
                    .handle_event(user_event)
                    .map_err(PushParseError::Handler)?;
                view.apply_unescaped_reset_if_queued();
            }
            EventResult::ExtractNumber(from_container_end) => {
                let user_event = view.validate_and_extract_number(from_container_end)?;
                handler
                    .handle_event(user_event)
                    .map_err(PushParseError::Handler)?;
                view.apply_unescaped_reset_if_queued();
            }
            EventResult::Continue => {}
        }
        return Ok(());
    }

    match event {
        ujson::Event::Begin(EventToken::Key) => {
            let pos = view.current_position();
            *view.parser_state_mut() = State::Key(pos);
            view.begin_string_content(pos);
        }
        ujson::Event::Begin(EventToken::String) => {
            let pos = view.current_position();
            *view.parser_state_mut() = State::String(pos);
            view.begin_string_content(pos);
        }
        ujson::Event::Begin(
            EventToken::Number | EventToken::NumberAndArray | EventToken::NumberAndObject,
        ) => {
            // The event position is the number's first byte; State stores
            // one before it (ContentRange convention shared by all parsers).
            let pos = view.current_position();
            *view.parser_state_mut() = State::Number(ContentRange::number_start_from_current(pos));
            view.begin_number_span(pos);
        }
        ujson::Event::Begin(EventToken::True | EventToken::False | EventToken::Null) => {}
        ujson::Event::Begin(EventToken::EscapeSequence) => {
            view.process_begin_escape_sequence_event()?;
        }
        ujson::Event::Begin(EventToken::UnicodeEscape)
        | ujson::Event::End(EventToken::UnicodeEscape) => {
            view.process_unicode_escape_events(&event)?;
        }
        ujson::Event::End(
            ref escape_token @ (EventToken::EscapeQuote
            | EventToken::EscapeBackslash
            | EventToken::EscapeSlash
            | EventToken::EscapeBackspace
            | EventToken::EscapeFormFeed
            | EventToken::EscapeNewline
            | EventToken::EscapeCarriageReturn
            | EventToken::EscapeTab),
        ) => {
            // OnEnd timing, as the byte-wise drive used for PushParser.
            view.process_simple_escape_event(escape_token)?;
        }
        _ => {}
    }
    Ok(())
}

/// An error that can occur during push-based parsing.
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PushParseError<E> {
    /// An error occurred within the parser itself.
    Parse(ParseError),
    /// An error was returned by the user's handler.
    Handler(E),
}

impl<E> From<ujson::Error> for PushParseError<E> {
    fn from(e: ujson::Error) -> Self {
        PushParseError::Parse(e.into())
    }
}

impl<E> From<ParseError> for PushParseError<E> {
    fn from(e: ParseError) -> Self {
        PushParseError::Parse(e)
    }
}

impl<E> From<StreamBufferError> for PushParseError<E> {
    fn from(e: StreamBufferError) -> Self {
        PushParseError::Parse(e.into())
    }
}

impl<E> From<core::str::Utf8Error> for PushParseError<E> {
    fn from(e: core::str::Utf8Error) -> Self {
        PushParseError::Parse(ParseError::InvalidUtf8(e))
    }
}
