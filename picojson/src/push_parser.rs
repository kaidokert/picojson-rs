// SPDX-License-Identifier: Apache-2.0

//! A SAX-style, `no_std` JSON push parser.
//!
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::escape_processor::UnicodeEscapeCollector;
use crate::event_processor::ParserCore;
use crate::stream_buffer::{StreamBuffer, StreamBufferError};
use crate::{ujson, BitStackConfig, Event, JsonNumber, ParseError, String};

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

/// A trait for handling events from a SAX-style push parser.
pub trait PushParserHandler<'input, 'scratch, E> {
    /// Handles a single, complete JSON event.
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), E>;
}

/// A SAX-style, `no_std` JSON push parser.
pub struct PushParser<'scratch, H, C>
where
    C: BitStackConfig,
{
    handler: H,
    stream_buffer: StreamBuffer<'scratch>,
    core: ParserCore<C::Bucket, C::Counter>,
    unicode_escape_collector: UnicodeEscapeCollector,
    state: ParserState,
    escape_state: EscapeState,
    position_offset: usize,
    current_position: usize,
    token_start_pos: usize,
    using_unescaped_buffer: bool,
}

impl<'scratch, H, C> PushParser<'scratch, H, C>
where
    C: BitStackConfig,
{
    /// Creates a new `PushParser`.
    pub fn new(handler: H, buffer: &'scratch mut [u8]) -> Self {
        Self {
            handler,
            stream_buffer: StreamBuffer::new(buffer),
            core: ParserCore::new(),
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            state: ParserState::Idle,
            escape_state: EscapeState::None,
            position_offset: 0,
            current_position: 0,
            token_start_pos: 0,
            using_unescaped_buffer: false,
        }
    }

    /// Processes a chunk of input data.
    pub fn write<'input, E>(&mut self, data: &'input [u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        let mut event_storage: [Option<(ujson::Event, usize)>; 2] = [None, None];

        for (local_pos, &byte) in data.iter().enumerate() {
            // Update current position to absolute position
            self.current_position = self.position_offset + local_pos;
            let mut append_byte = true;

            // Tokenizer generates events that drive state transitions.
            {
                let mut callback =
                    create_tokenizer_callback(&mut event_storage, self.current_position);
                self.core.tokenizer.parse_chunk(&[byte], &mut callback)?;
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
                                if self.using_unescaped_buffer {
                                    self.stream_buffer.append_unescaped_byte(byte)?;
                                }
                            }
                            ParserState::Idle => {}
                        }
                    }
                    EscapeState::InUnicodeEscape => {
                        // Feed hex digits to the unicode escape collector
                        match self.state {
                            ParserState::ParsingString | ParserState::ParsingKey => {
                                // Always feed hex digits to collector when in unicode escape state
                                match self.unicode_escape_collector.add_hex_digit(byte) {
                                    Ok(is_complete) => {
                                        if is_complete {
                                            // We have all 4 hex digits, process immediately
                                            // Ensure we're in unescaped mode since we have processed content
                                            if !self.using_unescaped_buffer {
                                                self.using_unescaped_buffer = true;
                                                self.stream_buffer.clear_unescaped();
                                            }

                                            let mut utf8_buffer = [0u8; 4];
                                            match self
                                                .unicode_escape_collector
                                                .process_to_utf8(&mut utf8_buffer)
                                            {
                                                Ok((utf8_bytes, _)) => {
                                                    if let Some(bytes) = utf8_bytes {
                                                        for &b in bytes {
                                                            self.stream_buffer
                                                                .append_unescaped_byte(b)?;
                                                        }
                                                    }
                                                }
                                                Err(_e) => {}
                                            }
                                        }
                                    }
                                    Err(_e) => {}
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
            if !self.using_unescaped_buffer {
                self.switch_to_unescaped_mode(data, data.len())?;
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
    {
        // Handle any remaining content in the buffer
        if self.state == ParserState::ParsingNumber {
            let s = self.stream_buffer.get_unescaped_slice()?;
            let num = JsonNumber::from_slice(s)?;
            self.handler
                .handle_event(Event::Number(num))
                .map_err(PushParseError::Handler)?;
            self.stream_buffer.clear_unescaped();
        }

        // Check that the JSON document is complete (all containers closed)
        // Use a no-op callback since we don't expect any more events
        let mut no_op_callback = |_event: ujson::Event, _pos: usize| {};
        self.core.tokenizer.finish(&mut no_op_callback)?;

        self.handler
            .handle_event(Event::EndDocument)
            .map_err(PushParseError::Handler)
    }

    /// Destroys the parser and returns the handler.
    pub fn destroy(self) -> H {
        self.handler
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
            ParserState::Idle => match event {
                ujson::Event::Begin(ujson::EventToken::String) => {
                    should_append = false;
                    self.token_start_pos = pos;
                    self.using_unescaped_buffer = false;
                    self.stream_buffer.clear_unescaped();
                    ParserState::ParsingString
                }
                ujson::Event::Begin(ujson::EventToken::Key) => {
                    should_append = false;
                    self.token_start_pos = pos;
                    self.using_unescaped_buffer = false;
                    self.stream_buffer.clear_unescaped();
                    ParserState::ParsingKey
                }
                ujson::Event::Begin(ujson::EventToken::Number) => {
                    self.token_start_pos = pos;
                    self.using_unescaped_buffer = false;
                    self.stream_buffer.clear_unescaped();
                    ParserState::ParsingNumber
                }
                ujson::Event::ObjectStart => {
                    self.handler
                        .handle_event(Event::StartObject)
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ObjectEnd => {
                    self.handler
                        .handle_event(Event::EndObject)
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ArrayStart => {
                    self.handler
                        .handle_event(Event::StartArray)
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ArrayEnd => {
                    self.handler
                        .handle_event(Event::EndArray)
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::True) => {
                    self.handler
                        .handle_event(Event::Bool(true))
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::False) => {
                    self.handler
                        .handle_event(Event::Bool(false))
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::Null) => {
                    self.handler
                        .handle_event(Event::Null)
                        .map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                _ => ParserState::Idle,
            },
            ParserState::ParsingString => {
                match event {
                    ujson::Event::End(ujson::EventToken::String) => {
                        should_append = false;
                        if self.using_unescaped_buffer {
                            let s = self.stream_buffer.get_unescaped_slice()?;
                            let s_str = core::str::from_utf8(s)?;
                            self.handler
                                .handle_event(Event::String(String::Unescaped(s_str)))
                                .map_err(PushParseError::Handler)?;
                        } else {
                            let s_str = self.extract_borrowed_content(data)?;
                            self.handler
                                .handle_event(Event::String(String::Borrowed(s_str)))
                                .map_err(PushParseError::Handler)?;
                        }
                        self.stream_buffer.clear_unescaped();
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
            ParserState::ParsingKey => {
                match event {
                    ujson::Event::End(ujson::EventToken::Key) => {
                        should_append = false;
                        if self.using_unescaped_buffer {
                            let s = self.stream_buffer.get_unescaped_slice()?;
                            let s_str = core::str::from_utf8(s)?;
                            self.handler
                                .handle_event(Event::Key(String::Unescaped(s_str)))
                                .map_err(PushParseError::Handler)?;
                        } else {
                            let s_str = self.extract_borrowed_content(data)?;
                            self.handler
                                .handle_event(Event::Key(String::Borrowed(s_str)))
                                .map_err(PushParseError::Handler)?;
                        }
                        self.stream_buffer.clear_unescaped();
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
                    if self.using_unescaped_buffer {
                        let s = self.stream_buffer.get_unescaped_slice()?;
                        let num = JsonNumber::from_slice(s)?;
                        self.handler
                            .handle_event(Event::Number(num))
                            .map_err(PushParseError::Handler)?;
                    } else {
                        let end_pos = self.current_position;
                        let start_pos = self.token_start_pos;
                        if end_pos >= start_pos {
                            let s_bytes = &data[(start_pos - self.position_offset)
                                ..(end_pos - self.position_offset)];
                            let num = JsonNumber::from_slice(s_bytes)?;
                            self.handler
                                .handle_event(Event::Number(num))
                                .map_err(PushParseError::Handler)?;
                        }
                    }
                    self.stream_buffer.clear_unescaped();
                    should_append = false;
                    ParserState::Idle
                }
                _ => ParserState::ParsingNumber,
            },
        };
        Ok((new_state, should_append))
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
                // This marks the beginning of an escape sequence - suppress raw byte appending
                self.escape_state = EscapeState::InEscapeSequence;
                // Switch to unescaped mode if not already, but don't include the backslash
                if !self.using_unescaped_buffer {
                    // Copy content up to (but not including) the backslash
                    let start_pos = self.token_start_pos + 1;
                    let end_pos = pos; // Don't include the backslash at 'pos'
                    if end_pos > start_pos {
                        self.using_unescaped_buffer = true;
                        self.stream_buffer.clear_unescaped();
                        let initial_part = &data
                            [(start_pos - self.position_offset)..(end_pos - self.position_offset)];
                        for &byte in initial_part {
                            self.stream_buffer.append_unescaped_byte(byte)?;
                        }
                    } else {
                        self.using_unescaped_buffer = true;
                        self.stream_buffer.clear_unescaped();
                    }
                }
            }
            ujson::Event::Begin(ujson::EventToken::UnicodeEscape) => {
                // Start of unicode escape sequence - reset collector for new sequence
                self.unicode_escape_collector.reset();
                self.escape_state = EscapeState::InUnicodeEscape;
                // Force switch to unescaped mode since we'll need to write processed unicode content
                if !self.using_unescaped_buffer {
                    self.using_unescaped_buffer = true;
                    self.stream_buffer.clear_unescaped();
                    // Copy any content that was accumulated before this unicode escape
                    let start_pos = self.token_start_pos + 1;
                    let end_pos = self.current_position;
                    if end_pos > start_pos {
                        let initial_part = &data
                            [(start_pos - self.position_offset)..(end_pos - self.position_offset)];
                        for &byte in initial_part {
                            self.stream_buffer.append_unescaped_byte(byte)?;
                        }
                    }
                }
            }
            ujson::Event::End(ujson::EventToken::EscapeQuote) => {
                // Switch to unescaped mode and append the escaped quote
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'"')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeBackslash) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\\')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeSlash) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'/')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeBackspace) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\x08')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeFormFeed) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\x0C')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeNewline) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\n')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeCarriageReturn) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\r')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::EscapeTab) => {
                if !self.using_unescaped_buffer {
                    self.switch_to_unescaped_mode(data, pos)?;
                }
                self.stream_buffer.append_unescaped_byte(b'\t')?;
                self.escape_state = EscapeState::None;
            }
            ujson::Event::End(ujson::EventToken::UnicodeEscape) => {
                // Important: Don't append the current byte since it's the 4th hex digit
                // that we'll process manually

                // Special case: if we're processing the End event during a hex digit byte,
                // we need to process that final hex digit first
                let current_byte = data.get(pos - self.position_offset).copied();
                if let Some(byte) = current_byte {
                    if byte.is_ascii_hexdigit() {
                        match self.unicode_escape_collector.add_hex_digit(byte) {
                            Ok(is_complete) => {
                                if is_complete {
                                    // Ensure we're in unescaped mode (but don't clear if already in unescaped mode)
                                    if !self.using_unescaped_buffer {
                                        self.using_unescaped_buffer = true;
                                        self.stream_buffer.clear_unescaped();

                                        // Copy any content that was accumulated before this unicode escape
                                        let start_pos = self.token_start_pos + 1;
                                        let unicode_escape_start = pos - 5; // \u0041 -> 5 chars back (\u + 4 hex digits)
                                        if unicode_escape_start > start_pos {
                                            let initial_part = &data[(start_pos
                                                - self.position_offset)
                                                ..(unicode_escape_start - self.position_offset)];
                                            for &byte in initial_part {
                                                self.stream_buffer.append_unescaped_byte(byte)?;
                                            }
                                        }
                                    }

                                    let mut utf8_buffer = [0u8; 4];
                                    match self
                                        .unicode_escape_collector
                                        .process_to_utf8(&mut utf8_buffer)
                                    {
                                        Ok((utf8_bytes, _)) => {
                                            if let Some(bytes) = utf8_bytes {
                                                for &b in bytes {
                                                    self.stream_buffer.append_unescaped_byte(b)?;
                                                }
                                            }
                                        }
                                        Err(_e) => {}
                                    }
                                }
                            }
                            Err(_e) => {}
                        }
                    }
                }

                // Process the collected unicode escape (now this will just reset the collector)
                self.process_collected_unicode_escape()?;
                self.escape_state = EscapeState::None;
            }
            _ => {}
        }
        Ok(self.state)
    }

    fn switch_to_unescaped_mode<E>(
        &mut self,
        data: &[u8],
        current_local_pos: usize,
    ) -> Result<(), PushParseError<E>> {
        if !self.using_unescaped_buffer {
            let start_pos = self.token_start_pos + 1;
            let end_pos = self.position_offset + current_local_pos;
            if end_pos > start_pos {
                // Only switch to unescaped mode if there's actually content to copy
                self.using_unescaped_buffer = true;
                let initial_part =
                    &data[(start_pos - self.position_offset)..(end_pos - self.position_offset)];
                for &byte in initial_part {
                    self.stream_buffer.append_unescaped_byte(byte)?;
                }
            }
            // Note: If there's no initial content, we stay in borrowed mode until we actually
            // need to write escaped content to the buffer
        }
        Ok(())
    }

    fn extract_borrowed_content<'a, E>(
        &self,
        data: &'a [u8],
    ) -> Result<&'a str, PushParseError<E>> {
        let start_pos = self.token_start_pos + 1;
        let end_pos = self.current_position;
        if end_pos >= start_pos {
            let s_bytes =
                &data[(start_pos - self.position_offset)..(end_pos - self.position_offset)];
            Ok(core::str::from_utf8(s_bytes)?)
        } else {
            Ok("")
        }
    }

    /// Process the collected unicode escape using the collector's state
    fn process_collected_unicode_escape<E>(&mut self) -> Result<(), PushParseError<E>> {
        // Unicode processing is now handled in the byte loop when hex digits are processed
        // This method just resets the collector for the next escape sequence
        self.unicode_escape_collector.reset();
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
        PushParseError::Parse(ParseError::Utf8(e))
    }
}

fn create_tokenizer_callback(
    event_storage: &mut [Option<(ujson::Event, usize)>; 2],
    position_offset: usize,
) -> impl FnMut(ujson::Event, usize) + '_ {
    move |event, _pos| {
        for evt in event_storage.iter_mut() {
            if evt.is_none() {
                *evt = Some((event, position_offset));
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
