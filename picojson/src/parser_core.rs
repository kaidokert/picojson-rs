// SPDX-License-Identifier: Apache-2.0

//! Unified parser core that handles the common event processing loop.
//!
//! This module provides the `ParserCore` struct that consolidates the shared
//! event processing logic between SliceParser and StreamParser, eliminating
//! the duplication in their `next_event_impl` methods.

use crate::event_processor::{
    finish_tokenizer, have_events, process_begin_escape_sequence_event, process_begin_events,
    process_byte_through_tokenizer, process_simple_escape_event, process_simple_events,
    process_unicode_escape_events, take_first_event, ContentExtractor, EventResult,
};
use crate::shared::{ByteProvider, Event, ParserState, UnexpectedState};
use crate::ujson::{EventToken, Tokenizer};
use crate::{ujson, ParseError};

/// Combined trait for parsers that provide both byte access and content extraction
pub trait ParserProvider: ByteProvider + ContentExtractor {}
impl<T: ByteProvider + ContentExtractor> ParserProvider for T {}

/// The core parser logic that handles the unified event processing loop.
///
/// This struct contains all the shared state and logic that was previously
/// duplicated between SliceParser and StreamParser. It uses trait abstractions
/// to handle the differences in content building and byte providing.
pub struct ParserCore<T: ujson::BitBucket, C: ujson::DepthCounter> {
    /// The tokenizer that processes JSON tokens
    pub tokenizer: Tokenizer<T, C>,
    /// Parser state and event storage
    pub parser_state: ParserState,
}

impl<T: ujson::BitBucket, C: ujson::DepthCounter> ParserCore<T, C> {
    /// Create a new ParserCore
    pub fn new() -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            parser_state: ParserState::new(),
        }
    }

    /// Unified implementation that works with a single combined provider.
    /// This avoids borrowing conflicts by using a single object that implements both traits.
    pub fn next_event_impl_unified<'a, P>(
        &mut self,
        provider: &'a mut P,
        escape_timing: EscapeTiming,
    ) -> Result<Event<'a, 'a>, ParseError>
    where
        P: ParserProvider,
    {
        self.next_event_impl_unified_with_accumulator(provider, escape_timing, |_, _| Ok(()))
    }

    /// Unified implementation with optional byte accumulation callback.
    /// This supports StreamParser-specific byte accumulation when no events are generated.
    pub fn next_event_impl_unified_with_accumulator<'a, P, F>(
        &mut self,
        provider: &'a mut P,
        escape_timing: EscapeTiming,
        mut byte_accumulator: F,
    ) -> Result<Event<'a, 'a>, ParseError>
    where
        P: ParserProvider,
        F: FnMut(&mut P, u8) -> Result<(), ParseError>,
    {
        loop {
            while !have_events(&self.parser_state.evts) {
                if let Some(byte) = provider.next_byte()? {
                    process_byte_through_tokenizer(
                        byte,
                        &mut self.tokenizer,
                        &mut self.parser_state.evts,
                    )?;

                    // Call byte accumulator if no events were generated (StreamParser-specific)
                    if !have_events(&self.parser_state.evts) {
                        byte_accumulator(provider, byte)?;
                    }
                } else {
                    // Handle end of stream - let the provider handle any cleanup
                    // For StreamParser, this is where finished flag gets set
                    finish_tokenizer(&mut self.tokenizer, &mut self.parser_state.evts)?;

                    if !have_events(&self.parser_state.evts) {
                        return Ok(Event::EndDocument);
                    }
                }
            }

            let taken_event = take_first_event(&mut self.parser_state.evts);
            let Some(taken) = taken_event else {
                return Err(UnexpectedState::StateMismatch.into());
            };

            // Try shared event processors first
            if let Some(result) =
                process_simple_events(&taken).or_else(|| process_begin_events(&taken, provider))
            {
                match result {
                    EventResult::Complete(event) => return Ok(event),
                    EventResult::ExtractString => return provider.validate_and_extract_string(),
                    EventResult::ExtractKey => return provider.validate_and_extract_key(),
                    EventResult::ExtractNumber(from_container_end) => {
                        return provider.validate_and_extract_number(from_container_end)
                    }
                    EventResult::Continue => continue,
                }
            }

            // Handle parser-specific events based on escape timing
            match taken {
                ujson::Event::Begin(EventToken::EscapeSequence) => {
                    process_begin_escape_sequence_event(provider)?;
                }
                _ if process_unicode_escape_events(&taken, provider)? => {
                    // Unicode escape events handled by shared function
                }
                ujson::Event::Begin(
                    escape_token @ (EventToken::EscapeQuote
                    | EventToken::EscapeBackslash
                    | EventToken::EscapeSlash
                    | EventToken::EscapeBackspace
                    | EventToken::EscapeFormFeed
                    | EventToken::EscapeNewline
                    | EventToken::EscapeCarriageReturn
                    | EventToken::EscapeTab),
                ) if escape_timing == EscapeTiming::OnBegin => {
                    // SliceParser-specific: Handle simple escape sequences on Begin events
                    // because CopyOnEscape requires starting unescaping immediately when
                    // the escape token begins to maintain zero-copy optimization
                    process_simple_escape_event(&escape_token, provider)?;
                }
                ujson::Event::End(
                    escape_token @ (EventToken::EscapeQuote
                    | EventToken::EscapeBackslash
                    | EventToken::EscapeSlash
                    | EventToken::EscapeBackspace
                    | EventToken::EscapeFormFeed
                    | EventToken::EscapeNewline
                    | EventToken::EscapeCarriageReturn
                    | EventToken::EscapeTab),
                ) if escape_timing == EscapeTiming::OnEnd => {
                    // StreamParser-specific: Handle simple escape sequences on End events
                    // because StreamBuffer must wait until the token ends to accumulate
                    // all bytes before processing the complete escape sequence
                    process_simple_escape_event(&escape_token, provider)?;
                }
                _ => {
                    // All other events continue to next iteration
                }
            }
        }
    }
}

impl<T: ujson::BitBucket, C: ujson::DepthCounter> Default for ParserCore<T, C> {
    fn default() -> Self {
        Self::new()
    }
}

/// Enum to specify when escape sequences should be processed
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EscapeTiming {
    /// Process simple escape sequences on Begin events (SliceParser)
    OnBegin,
    /// Process simple escape sequences on End events (StreamParser)
    OnEnd,
}
