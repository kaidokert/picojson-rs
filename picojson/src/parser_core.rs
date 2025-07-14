// SPDX-License-Identifier: Apache-2.0

//! Unified parser core that handles the common event processing loop.
//!
//! This module provides the `ParserCore` struct that consolidates the shared
//! event processing logic between SliceParser and StreamParser, eliminating
//! the duplication in their `next_event_impl` methods.

use crate::content_builder::ContentBuilder;
use crate::event_processor::{
    finish_tokenizer, have_events, process_begin_escape_sequence_event, process_begin_events,
    process_byte_through_tokenizer, process_simple_escape_event, process_simple_events,
    process_unicode_escape_events, take_first_event, EventResult,
};
use crate::shared::{ByteProvider, Event, ParserState, UnexpectedState};
use crate::ujson::{EventToken, Tokenizer};
use crate::{ujson, ParseError};

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

    /// The unified event processing loop that was previously duplicated
    /// between SliceParser and StreamParser.
    ///
    /// This method implements the common logic while delegating to traits
    /// for the parser-specific differences.
    pub fn next_event_impl<'a, B, CB>(
        &mut self,
        byte_provider: &mut B,
        content_builder: &'a mut CB,
        escape_timing: EscapeTiming,
    ) -> Result<Event<'a, 'a>, ParseError>
    where
        B: ByteProvider,
        CB: ContentBuilder,
    {
        loop {
            while !have_events(&self.parser_state.evts) {
                if !self.pull_tokenizer_events(byte_provider)? {
                    return Ok(Event::EndDocument);
                }
            }

            let taken_event = take_first_event(&mut self.parser_state.evts);
            let Some(taken) = taken_event else {
                return Err(UnexpectedState::StateMismatch.into());
            };

            // Try shared event processors first
            if let Some(result) = process_simple_events(&taken)
                .or_else(|| process_begin_events(&taken, content_builder))
            {
                match result {
                    EventResult::Complete(event) => return Ok(event),
                    EventResult::ExtractString => {
                        return content_builder.validate_and_extract_string()
                    }
                    EventResult::ExtractKey => return content_builder.validate_and_extract_key(),
                    EventResult::ExtractNumber(from_container_end) => {
                        return content_builder.validate_and_extract_number(from_container_end)
                    }
                    EventResult::Continue => continue,
                }
            }

            // Handle parser-specific events based on escape timing
            match taken {
                ujson::Event::Begin(EventToken::EscapeSequence) => {
                    process_begin_escape_sequence_event(content_builder)?;
                }
                _ if process_unicode_escape_events(&taken, content_builder)? => {
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
                    process_simple_escape_event(&escape_token, content_builder)?;
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
                    process_simple_escape_event(&escape_token, content_builder)?;
                }
                _ => {
                    // All other events continue to next iteration
                }
            }
        }
    }

    /// Pull events from tokenizer and return whether parsing should continue.
    /// This implements the common tokenizer event pulling logic.
    fn pull_tokenizer_events<B: ByteProvider>(
        &mut self,
        byte_provider: &mut B,
    ) -> Result<bool, ParseError> {
        if let Some(byte) = byte_provider.next_byte()? {
            process_byte_through_tokenizer(byte, &mut self.tokenizer, &mut self.parser_state.evts)?;
        } else {
            finish_tokenizer(&mut self.tokenizer, &mut self.parser_state.evts)?;

            if !have_events(&self.parser_state.evts) {
                return Ok(false); // Signal end of parsing
            }
        }
        Ok(true) // Continue parsing
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
