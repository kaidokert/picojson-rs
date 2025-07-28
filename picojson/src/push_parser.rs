// SPDX-License-Identifier: Apache-2.0

//! A SAX-style JSON push parser.
//!
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::event_processor::ContentExtractor;
use crate::push_content_builder::{PushContentBuilder, PushParserHandler};
use crate::stream_buffer::StreamBufferError;
use crate::{ujson, BitStackConfig, Event, ParseError};

#[cfg(any(test, debug_assertions))]
extern crate std;

/// Manages the parser's state.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ParserState {
    Idle,
    ParsingString,
    ParsingKey,
    ParsingNumber,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EscapeState {
    None,
    InEscapeSequence,
    InUnicodeEscape,
}

/// A SAX-style JSON push parser.
///
/// Generic over BitStack storage type for configurable nesting depth. Parsing
/// events are returned to the handler.
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
    /// Content builder that handles content extraction and event emission
    content_builder: PushContentBuilder<'scratch, H>,
    /// Core tokenizer for JSON processing
    tokenizer: ujson::Tokenizer<C::Bucket, C::Counter>,
    /// Current parser state
    state: ParserState,
    /// Current escape processing state
    escape_state: EscapeState,
    /// Position offset for tracking absolute positions across chunks
    position_offset: usize,
    /// Current position within the current chunk
    current_position: usize,
}

impl<'scratch, H, C> PushParser<'scratch, H, C>
where
    C: BitStackConfig,
{
    /// Creates a new `PushParser`.
    pub fn new(handler: H, buffer: &'scratch mut [u8]) -> Self {
        Self {
            content_builder: PushContentBuilder::new(handler, buffer),
            tokenizer: ujson::Tokenizer::new(),
            state: ParserState::Idle,
            escape_state: EscapeState::None,
            position_offset: 0,
            current_position: 0,
        }
    }

    /// Processes a chunk of input data.
    pub fn write<'input, E>(&mut self, data: &'input [u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Event storage for tokenizer output. Fixed size of 2 events is by design:
        // the tokenizer never returns more than 2 events per input byte. This is a
        // fundamental limit that all parsers in this codebase use.
        let mut event_storage: [Option<(ujson::Event, usize)>; 2] = [None, None];

        // Update content builder position information
        self.content_builder
            .set_position_info(self.position_offset, 0);

        // TODO: Consider optimizing byte-by-byte processing for better performance
        // Currently processes one byte at a time which may be suboptimal for large inputs
        for (local_pos, &byte) in data.iter().enumerate() {
            // Update current position to absolute position
            self.current_position = self.position_offset + local_pos;
            self.content_builder
                .set_position_info(self.position_offset, self.current_position);
            let mut append_byte = true;

            // Tokenizer generates events that drive state transitions.
            {
                let mut callback =
                    create_tokenizer_callback(&mut event_storage, self.current_position);
                let _bytes_processed = self.tokenizer.parse_chunk(&[byte], &mut callback)?;
            }

            while let Some((event, event_pos)) = take_first_event(&mut event_storage) {
                let (new_state, should_append) = self.handle_event(event, event_pos, data)?;
                self.state = new_state;
                if !should_append {
                    append_byte = false;
                }
            }

            // Handle byte processing based on escape state
            if append_byte {
                match self.escape_state {
                    EscapeState::None => {
                        // Normal content processing - append to buffer
                        match self.state {
                            ParserState::ParsingString
                            | ParserState::ParsingKey
                            | ParserState::ParsingNumber => {
                                if self.content_builder.is_using_unescaped_buffer() {
                                    self.content_builder.append_unescaped_byte(byte)?;
                                }
                            }
                            ParserState::Idle => {}
                        }
                    }
                    EscapeState::InUnicodeEscape => {
                        // Feed hex digits to the collector during Unicode escape processing
                        match self.state {
                            ParserState::ParsingString | ParserState::ParsingKey => {
                                // Feed hex digits to collector - let the End event handle UTF-8 conversion
                                match self
                                    .content_builder
                                    .unicode_escape_collector_mut()
                                    .add_hex_digit(byte)
                                {
                                    Ok(_is_complete) => {
                                        // Don't process immediately - let the End event handle the UTF-8 conversion
                                        // This avoids duplicate processing while still collecting hex digits
                                    }
                                    Err(e) => return Err(e.into()),
                                }
                            }
                            _ => {}
                        }
                    }
                    EscapeState::InEscapeSequence => {
                        // Other escape sequences are handled by events, not by byte processing
                    }
                }
            }
        }

        // Switch to unescaped mode for active content if needed
        if let ParserState::ParsingString | ParserState::ParsingKey | ParserState::ParsingNumber =
            self.state
        {
            if !self.content_builder.is_using_unescaped_buffer() {
                let state = match self.state {
                    ParserState::ParsingString => crate::shared::State::String(0), // Position will be corrected by content builder
                    ParserState::ParsingKey => crate::shared::State::Key(0),
                    ParserState::ParsingNumber => crate::shared::State::Number(0),
                    ParserState::Idle => crate::shared::State::None,
                };
                self.content_builder
                    .switch_to_unescaped_mode::<E>(data, data.len(), state)?;
            }
        }

        // Update position offset for next call
        self.position_offset += data.len();

        Ok(())
    }

    /// Finishes parsing and flushes any remaining events.
    pub fn finish<E>(&mut self) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Handle any remaining content in the buffer and check for incomplete parsing
        match self.state {
            ParserState::ParsingNumber => {
                // Numbers are automatically switched to unescaped mode at the end of each write() call,
                // so by the time we reach finish(), they should always be in the content builder
                self.content_builder
                    .handle_unfinished_number()
                    .map_err(PushParseError::Handler)?;
            }
            ParserState::ParsingString => {
                if self
                    .content_builder
                    .unicode_escape_collector_mut()
                    .is_in_progress()
                {
                    return Err(PushParseError::Parse(ParseError::InvalidUnicodeHex));
                }
                return Err(PushParseError::Parse(ParseError::EndOfData));
            }
            ParserState::ParsingKey => {
                if self
                    .content_builder
                    .unicode_escape_collector_mut()
                    .is_in_progress()
                {
                    return Err(PushParseError::Parse(ParseError::InvalidUnicodeHex));
                }
                return Err(PushParseError::Parse(ParseError::EndOfData));
            }
            ParserState::Idle => {}
        }

        // Check that the JSON document is complete (all containers closed)
        // Use a no-op callback since we don't expect any more events
        let mut no_op_callback = |_event: ujson::Event, _pos: usize| {};
        let _bytes_processed = self.tokenizer.finish(&mut no_op_callback)?;

        self.content_builder
            .emit_event(Event::EndDocument)
            .map_err(PushParseError::Handler)
    }

    /// Destroys the parser and returns the handler.
    pub fn destroy(self) -> H {
        self.content_builder.destroy()
    }

    /// Returns (new_state, should_append_byte)
    fn handle_event<'input, E>(
        &mut self,
        event: ujson::Event,
        pos: usize,
        data: &'input [u8],
    ) -> Result<(ParserState, bool), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        let mut should_append = true;
        let new_state = match self.state {
            ParserState::Idle => self.handle_idle_state_event(event, pos, &mut should_append)?,
            ParserState::ParsingString | ParserState::ParsingKey => {
                let is_key = self.state == ParserState::ParsingKey;
                let end_token = if is_key {
                    ujson::EventToken::Key
                } else {
                    ujson::EventToken::String
                };

                match event {
                    ujson::Event::End(token) if token == end_token => {
                        should_append = false;
                        self.content_builder
                            .handle_string_key_end(data, is_key)
                            .map_err(PushParseError::Handler)?;
                        self.content_builder.apply_unescaped_reset_if_queued();
                        ParserState::Idle
                    }
                    ujson::Event::End(ujson::EventToken::EscapeQuote)
                    | ujson::Event::End(ujson::EventToken::EscapeBackslash)
                    | ujson::Event::End(ujson::EventToken::EscapeSlash)
                    | ujson::Event::End(ujson::EventToken::EscapeBackspace)
                    | ujson::Event::End(ujson::EventToken::EscapeFormFeed)
                    | ujson::Event::End(ujson::EventToken::EscapeNewline)
                    | ujson::Event::End(ujson::EventToken::EscapeCarriageReturn)
                    | ujson::Event::End(ujson::EventToken::EscapeTab)
                    | ujson::Event::End(ujson::EventToken::UnicodeEscape) => {
                        should_append = false; // Don't append the escape trigger byte to buffer
                        self.append_escape_content(event, pos, data)?
                    }
                    _ => self.append_escape_content(event, pos, data)?,
                }
            }
            ParserState::ParsingNumber => match event {
                ujson::Event::End(ujson::EventToken::Number)
                | ujson::Event::End(ujson::EventToken::NumberAndArray)
                | ujson::Event::End(ujson::EventToken::NumberAndObject) => {
                    self.content_builder
                        .handle_number_end(data)
                        .map_err(PushParseError::Handler)?;
                    self.content_builder.apply_unescaped_reset_if_queued();
                    should_append = false;
                    ParserState::Idle
                }
                _ => ParserState::ParsingNumber,
            },
        };
        Ok((new_state, should_append))
    }

    /// Handle events when parser is in idle state
    fn handle_idle_state_event<E>(
        &mut self,
        event: ujson::Event,
        pos: usize,
        should_append: &mut bool,
    ) -> Result<ParserState, PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        match event {
            ujson::Event::Begin(ujson::EventToken::String) => {
                *should_append = false;
                self.content_builder.start_content_token(pos);
                Ok(ParserState::ParsingString)
            }
            ujson::Event::Begin(ujson::EventToken::Key) => {
                *should_append = false;
                self.content_builder.start_content_token(pos);
                Ok(ParserState::ParsingKey)
            }
            ujson::Event::Begin(ujson::EventToken::Number) => {
                self.content_builder.start_content_token(pos);
                Ok(ParserState::ParsingNumber)
            }
            ujson::Event::ObjectStart => {
                self.content_builder
                    .emit_event(Event::StartObject)
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::ObjectEnd => {
                self.content_builder
                    .emit_event(Event::EndObject)
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::ArrayStart => {
                self.content_builder
                    .emit_event(Event::StartArray)
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::ArrayEnd => {
                self.content_builder
                    .emit_event(Event::EndArray)
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::End(ujson::EventToken::True) => {
                self.content_builder
                    .emit_event(Event::Bool(true))
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::End(ujson::EventToken::False) => {
                self.content_builder
                    .emit_event(Event::Bool(false))
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            ujson::Event::End(ujson::EventToken::Null) => {
                self.content_builder
                    .emit_event(Event::Null)
                    .map_err(PushParseError::Handler)?;
                Ok(ParserState::Idle)
            }
            _ => Ok(ParserState::Idle),
        }
    }

    fn append_escape_content<'input, E>(
        &mut self,
        event: ujson::Event,
        pos: usize,
        data: &'input [u8],
    ) -> Result<ParserState, PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        match event {
            ujson::Event::Begin(ujson::EventToken::EscapeSequence) => {
                self.handle_begin_escape_sequence(pos, data)?;
            }
            ujson::Event::Begin(ujson::EventToken::UnicodeEscape) => {
                self.handle_begin_unicode_escape(data)?;
            }
            ujson::Event::End(
                token @ (ujson::EventToken::EscapeQuote
                | ujson::EventToken::EscapeBackslash
                | ujson::EventToken::EscapeSlash
                | ujson::EventToken::EscapeBackspace
                | ujson::EventToken::EscapeFormFeed
                | ujson::EventToken::EscapeNewline
                | ujson::EventToken::EscapeCarriageReturn
                | ujson::EventToken::EscapeTab),
            ) => {
                if !self.content_builder.is_using_unescaped_buffer() {
                    let state = match self.state {
                        ParserState::ParsingString => crate::shared::State::String(0),
                        ParserState::ParsingKey => crate::shared::State::Key(0),
                        ParserState::ParsingNumber => crate::shared::State::Number(0),
                        ParserState::Idle => crate::shared::State::None,
                    };
                    self.content_builder
                        .switch_to_unescaped_mode::<E>(data, pos, state)?;
                }
                let unescaped_char = match token {
                    ujson::EventToken::EscapeQuote => b'"',
                    ujson::EventToken::EscapeBackslash => b'\\',
                    ujson::EventToken::EscapeSlash => b'/',
                    ujson::EventToken::EscapeBackspace => b'\x08',
                    ujson::EventToken::EscapeFormFeed => b'\x0C',
                    ujson::EventToken::EscapeNewline => b'\n',
                    ujson::EventToken::EscapeCarriageReturn => b'\r',
                    ujson::EventToken::EscapeTab => b'\t',
                    _ => unreachable!(), // Covered by the outer match arm guard
                };
                self.content_builder.append_unescaped_byte(unescaped_char)?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::UnicodeEscape) => {
                // Switch to unescaped mode if not already active
                if !self.content_builder.is_using_unescaped_buffer() {
                    let state = match self.state {
                        ParserState::ParsingString => crate::shared::State::String(0),
                        ParserState::ParsingKey => crate::shared::State::Key(0),
                        ParserState::ParsingNumber => crate::shared::State::Number(0),
                        ParserState::Idle => crate::shared::State::None,
                    };
                    self.content_builder
                        .switch_to_unescaped_mode::<E>(data, pos, state)?;
                }

                // Ensure we have all 4 hex digits before processing
                // The 4th hex digit might be the current byte being processed
                let current_byte = data.get(pos - self.position_offset).copied();
                if let Some(byte) = current_byte {
                    if byte.is_ascii_hexdigit() {
                        // Feed the final hex digit if it hasn't been processed yet
                        match self
                            .content_builder
                            .unicode_escape_collector_mut()
                            .add_hex_digit(byte)
                        {
                            Ok(_is_complete) => {
                                // Continue to process the complete sequence
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                }

                // Process the collected unicode escape to UTF-8
                self.content_builder
                    .process_unicode_escape_with_collector()?;
                self.escape_state = EscapeState::None;
            }
            _ => {
                // Other events during string/key parsing (e.g., Begin(String), Begin(Key), byte content)
                // are handled by the normal parsing flow and don't require special escape processing.
                // This is expected behavior during normal tokenization - we only process escape-specific
                // events in this function, while other events are handled elsewhere in the state machine.
            }
        }
        Ok(self.state)
    }

    /// Handle the beginning of a regular escape sequence
    fn handle_begin_escape_sequence<E>(
        &mut self,
        pos: usize,
        data: &[u8],
    ) -> Result<(), PushParseError<E>>
    where
        E: From<ParseError>,
    {
        // This marks the beginning of an escape sequence - suppress raw byte appending
        self.escape_state = EscapeState::InEscapeSequence;
        // Switch to unescaped mode through content builder
        if !self.content_builder.is_using_unescaped_buffer() {
            let state = match self.state {
                ParserState::ParsingString => crate::shared::State::String(0),
                ParserState::ParsingKey => crate::shared::State::Key(0),
                ParserState::ParsingNumber => crate::shared::State::Number(0),
                ParserState::Idle => crate::shared::State::None,
            };
            // For simple escapes, we want to copy content up to the backslash, not including it
            let token_start_pos = self.content_builder.get_token_start_pos();
            let content_start_pos = token_start_pos + 1; // Skip opening quote
            let backslash_pos = pos; // The pos parameter should be the backslash position
            if backslash_pos > content_start_pos {
                // There's content before the backslash
                // Pass the local position relative to current chunk, not absolute position
                let local_backslash_pos = backslash_pos - self.position_offset;
                self.content_builder.switch_to_unescaped_mode::<E>(
                    data,
                    local_backslash_pos,
                    state,
                )?;
            } else {
                // No previous content, just mark as using unescaped buffer
                self.content_builder.mark_using_unescaped_buffer();
            }
        }
        Ok(())
    }

    /// Handle the beginning of a Unicode escape sequence
    fn handle_begin_unicode_escape<E>(&mut self, data: &[u8]) -> Result<(), PushParseError<E>>
    where
        E: From<ParseError>,
    {
        // Start of unicode escape sequence - reset collector for new sequence
        self.content_builder.unicode_escape_collector_mut().reset();
        self.escape_state = EscapeState::InUnicodeEscape;
        // Force switch to unescaped mode since we'll need to write processed unicode content
        if !self.content_builder.is_using_unescaped_buffer() {
            let state = match self.state {
                ParserState::ParsingString => crate::shared::State::String(0),
                ParserState::ParsingKey => crate::shared::State::Key(0),
                ParserState::ParsingNumber => crate::shared::State::Number(0),
                ParserState::Idle => crate::shared::State::None,
            };
            // For Unicode escapes, we want to copy content up to the backslash, not including \u
            // The current_position points to the first hex digit, so we need to go back 2 positions to get the backslash
            let backslash_pos = self.current_position.saturating_sub(2); // Position of backslash
                                                                         // Only copy if there's actual content before the backslash
            let token_start_pos = self.content_builder.get_token_start_pos();
            let content_start_pos = token_start_pos + 1; // Skip opening quote
            if backslash_pos > content_start_pos {
                // There's content before the backslash
                self.content_builder
                    .switch_to_unescaped_mode::<E>(data, backslash_pos, state)?;
            } else {
                // No previous content, just mark as using unescaped buffer
                self.content_builder.mark_using_unescaped_buffer();
            }
        }
        Ok(())
    }
}

/// An error that can occur during push-based parsing.
#[derive(Debug, PartialEq)]
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

fn create_tokenizer_callback(
    event_storage: &mut [Option<(ujson::Event, usize)>; 2],
    base_position: usize,
) -> impl FnMut(ujson::Event, usize) + '_ {
    move |event, relative_pos| {
        for evt in event_storage.iter_mut() {
            if evt.is_none() {
                // Convert relative position from tokenizer to absolute position
                // base_position is the absolute position where this chunk starts
                // relative_pos is the position within this chunk (0 for single-byte processing)
                *evt = Some((event, base_position + relative_pos));
                return;
            }
        }
    }
}

fn take_first_event(
    event_storage: &mut [Option<(ujson::Event, usize)>; 2],
) -> Option<(ujson::Event, usize)> {
    event_storage.iter_mut().find_map(|e| e.take())
}

// Implement From<ParseError> for common error types used in tests
// This needs to be globally accessible for integration tests, not just unit tests
#[cfg(any(test, debug_assertions))]
impl From<ParseError> for std::string::String {
    fn from(_: ParseError) -> Self {
        std::string::String::new()
    }
}

impl From<ParseError> for () {
    fn from(_: ParseError) -> Self {
        ()
    }
}
