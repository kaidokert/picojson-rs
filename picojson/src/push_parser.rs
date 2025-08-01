// SPDX-License-Identifier: Apache-2.0

//! A SAX-style JSON push parser.
//!
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::push_content_builder::{PushContentBuilder, PushParserHandler};
use crate::shared::DataSource;
use crate::stream_buffer::{StreamBuffer, StreamBufferError};
use crate::{ujson, BitStackConfig, Event, ParseError};

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

/// Helper struct that implements DataSource for PushParser content extraction
pub struct PushParserDataSource<'input, 'scratch> {
    /// Current input data chunk being processed
    pub input_data: &'input [u8],
    /// Reference to the stream buffer for unescaped content
    pub stream_buffer: &'scratch StreamBuffer<'scratch>,
    /// Whether unescaped content is being used
    pub using_unescaped_buffer: bool,
    /// Position offset for converting absolute positions to slice positions
    pub position_offset: usize,
}

impl<'input, 'scratch> DataSource<'input, 'scratch> for PushParserDataSource<'input, 'scratch> {
    fn get_borrowed_slice(
        &'input self,
        start: usize,
        end: usize,
    ) -> Result<&'input [u8], ParseError> {
        // Convert absolute positions to relative positions within the current data chunk
        let slice_start = start.saturating_sub(self.position_offset);
        let slice_end = end.saturating_sub(self.position_offset);

        if slice_end > self.input_data.len() || slice_start > slice_end {
            return Err(ParseError::Unexpected(
                crate::shared::UnexpectedState::InvalidSliceBounds,
            ));
        }

        Ok(&self.input_data[slice_start..slice_end])
    }

    fn get_unescaped_slice(&'scratch self) -> Result<&'scratch [u8], ParseError> {
        self.stream_buffer
            .get_unescaped_slice()
            .map_err(ParseError::from)
    }

    fn has_unescaped_content(&self) -> bool {
        self.using_unescaped_buffer && self.stream_buffer.has_unescaped_content()
    }
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
    /// Current position within the current chunk
    current_position: usize,
    /// Parser state tracking
    state: ParserState,
    /// Escape state tracking
    escape_state: EscapeState,
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
            current_position: 0,
            state: ParserState::Idle,
            escape_state: EscapeState::None,
        }
    }

    /// Processes a chunk of input data.
    pub fn write<'input, E>(&mut self, data: &'input [u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Event storage for tokenizer output. Fixed size of 2 events is by design:
        // the tokenizer never returns more than 2 events per input byte. This is a
        // fundamental limit that all parsers in this codebase use.
        let mut event_storage: [Option<(ujson::Event, usize)>; 2] = [None, None];

        // TODO: Consider optimizing byte-by-byte processing for better performance
        // Currently processes one byte at a time which may be suboptimal for large inputs
        for (local_pos, &byte) in data.iter().enumerate() {
            // Update current position to absolute position
            self.current_position = self.content_builder.position_offset() + local_pos;
            self.content_builder
                .set_current_position(self.current_position);
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
                                if self.content_builder.using_unescaped_buffer() {
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

        if let ParserState::ParsingString | ParserState::ParsingKey | ParserState::ParsingNumber =
            self.state
        {
            if !self.content_builder.using_unescaped_buffer() {
                self.switch_to_unescaped_mode(data, data.len())?;
            }
        }

        // Update position offset for next call
        self.content_builder.add_position_offset(data.len());

        Ok(())
    }

    /// Finishes parsing and flushes any remaining events.
    pub fn finish<E>(&mut self) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Handle any remaining content in the buffer and check for incomplete parsing
        match self.state {
            ParserState::ParsingNumber => {
                // Numbers are automatically switched to unescaped mode at the end of each write() call,
                // so by the time we reach finish(), they should always be in the stream buffer
                if !self.content_builder.using_unescaped_buffer() {
                    return Err(PushParseError::Parse(ParseError::Unexpected(
                        crate::shared::UnexpectedState::StateMismatch,
                    )));
                }

                // Emit number from unescaped buffer atomically
                self.content_builder.emit_number_from_unescaped_buffer()?;
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
        self.content_builder.into_handler()
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

                        // The start position for string content is after the opening quote.
                        let content_start_pos = self.content_builder.token_start_pos() + 1;

                        // `get_content_piece` expects the end position to be *after* the delimiter,
                        // but the tokenizer gives us the position *of* the delimiter (`pos`), so we add 1.
                        self.content_builder.emit_string_or_key_event(
                            is_key,
                            data,
                            content_start_pos,
                            pos + 1,
                        )?;
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
                    should_append = false;

                    self.content_builder.emit_number_event(
                        data,
                        self.content_builder.token_start_pos(),
                        pos + 1,
                    )?;
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
    {
        match event {
            ujson::Event::Begin(ujson::EventToken::String) => {
                *should_append = false;
                self.content_builder.set_token_start_pos(pos);
                self.content_builder.set_using_unescaped_buffer(false);
                self.content_builder.clear_unescaped();
                Ok(ParserState::ParsingString)
            }
            ujson::Event::Begin(ujson::EventToken::Key) => {
                *should_append = false;
                self.content_builder.set_token_start_pos(pos);
                self.content_builder.set_using_unescaped_buffer(false);
                self.content_builder.clear_unescaped();
                Ok(ParserState::ParsingKey)
            }
            ujson::Event::Begin(ujson::EventToken::Number) => {
                self.content_builder.set_token_start_pos(pos);
                self.content_builder.set_using_unescaped_buffer(false);
                self.content_builder.clear_unescaped();
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
                if !self.content_builder.using_unescaped_buffer() {
                    self.switch_to_unescaped_mode(data, pos)?;
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
                if !self.content_builder.using_unescaped_buffer() {
                    self.switch_to_unescaped_mode(data, pos)?;
                }

                // Ensure we have all 4 hex digits before processing
                // The 4th hex digit might be the current byte being processed
                let current_byte = data
                    .get(pos - self.content_builder.position_offset())
                    .copied();
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
                let mut utf8_buffer = [0u8; 4];
                match self
                    .content_builder
                    .unicode_escape_collector_mut()
                    .process_to_utf8(&mut utf8_buffer)
                {
                    Ok((utf8_bytes, _)) => {
                        if let Some(bytes) = utf8_bytes {
                            for &b in bytes {
                                self.content_builder.append_unescaped_byte(b)?;
                            }
                        }
                    }
                    Err(e) => return Err(e.into()),
                }

                // Reset for next escape sequence
                self.content_builder.unicode_escape_collector_mut().reset();
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
    ) -> Result<(), PushParseError<E>> {
        // This marks the beginning of an escape sequence - suppress raw byte appending
        self.escape_state = EscapeState::InEscapeSequence;
        // Switch to unescaped mode if not already, but don't include the backslash
        if !self.content_builder.using_unescaped_buffer() {
            // Copy content up to (but not including) the backslash
            let start_pos = self.content_builder.token_start_pos() + 1;
            let end_pos = pos; // Don't include the backslash at 'pos'
            if end_pos > start_pos {
                self.content_builder.set_using_unescaped_buffer(true);
                self.content_builder.clear_unescaped();
                let initial_part = &data[(start_pos - self.content_builder.position_offset())
                    ..(end_pos - self.content_builder.position_offset())];
                for &byte in initial_part {
                    self.content_builder.append_unescaped_byte(byte)?;
                }
            } else {
                self.content_builder.set_using_unescaped_buffer(true);
                self.content_builder.clear_unescaped();
            }
        }
        Ok(())
    }

    /// Handle the beginning of a Unicode escape sequence
    fn handle_begin_unicode_escape<E>(&mut self, data: &[u8]) -> Result<(), PushParseError<E>> {
        // Start of unicode escape sequence - reset collector for new sequence
        self.content_builder.unicode_escape_collector_mut().reset();
        self.escape_state = EscapeState::InUnicodeEscape;
        // Force switch to unescaped mode since we'll need to write processed unicode content
        if !self.content_builder.using_unescaped_buffer() {
            self.content_builder.set_using_unescaped_buffer(true);
            self.content_builder.clear_unescaped();
            // Copy any content that was accumulated before this unicode escape
            let start_pos = self.content_builder.token_start_pos() + 1;
            let end_pos = self.current_position;
            if end_pos > start_pos {
                let initial_part = &data[(start_pos - self.content_builder.position_offset())
                    ..(end_pos - self.content_builder.position_offset())];
                for &byte in initial_part {
                    self.content_builder.append_unescaped_byte(byte)?;
                }
            }
        }
        Ok(())
    }

    fn switch_to_unescaped_mode<E>(
        &mut self,
        data: &[u8],
        current_local_pos: usize,
    ) -> Result<(), PushParseError<E>> {
        if !self.content_builder.using_unescaped_buffer() {
            // For strings/keys: skip opening quote (+1)
            // For numbers: start from first digit (+0)
            let start_offset = match self.state {
                ParserState::ParsingString | ParserState::ParsingKey => 1, // Skip opening quote
                ParserState::ParsingNumber => 0,                           // Include first digit
                ParserState::Idle => 0,
            };
            let start_pos = self.content_builder.token_start_pos() + start_offset;
            let end_pos = self.content_builder.position_offset() + current_local_pos;

            if end_pos > start_pos {
                // Only switch to unescaped mode if there's actually content to copy
                self.content_builder.set_using_unescaped_buffer(true);
                let slice_start = start_pos.saturating_sub(self.content_builder.position_offset());
                let slice_end = end_pos.saturating_sub(self.content_builder.position_offset());

                let initial_part = &data[slice_start..slice_end];
                for &byte in initial_part {
                    self.content_builder.append_unescaped_byte(byte)?;
                }
            }
            // Note: If there's no initial content, we stay in borrowed mode until we actually
            // need to write escaped content to the buffer
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
