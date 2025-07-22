// SPDX-License-Identifier: Apache-2.0

//! A SAX-style, `no_std` JSON push parser.
//! 
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::escape_processor::{process_unicode_escape_sequence, UnicodeEscapeCollector};
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
        }
    }

    /// Processes a chunk of input data.
    pub fn write<'input, E>(&mut self, data: &'input [u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        let mut event_storage: [Option<(ujson::Event, usize)>; 2] = [None, None];

        for (_pos, &byte) in data.iter().enumerate() {
            let mut append_byte = true;

            // Tokenizer generates events that drive state transitions.
            {
                let mut callback = create_tokenizer_callback(&mut event_storage);
                self.core.tokenizer.parse_chunk(&[byte], &mut callback)?;
            }

            while let Some((event, event_pos)) = take_first_event(&mut event_storage) {
                let (new_state, should_append) = self.handle_event(event, event_pos, data)?;
                self.state = new_state;
                if !should_append {
                    append_byte = false;
                }
            }

            // Append the raw byte if we are in a content-parsing state.
            if append_byte {
                match self.state {
                    ParserState::ParsingString | ParserState::ParsingKey | ParserState::ParsingNumber => {
                        self.stream_buffer.append_unescaped_byte(byte)?;
                    }
                    ParserState::Idle => {}
                }
            }
        }

        Ok(())
    }

    /// Finishes parsing and flushes any remaining events.
    pub fn finish<E>(&mut self) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
    {
        // Handle any remaining content in the buffer
        match self.state {
            ParserState::ParsingNumber => {
                let s = self.stream_buffer.get_unescaped_slice()?;
                let num = JsonNumber::from_slice(s)?;
                self.handler.handle_event(Event::Number(num)).map_err(PushParseError::Handler)?;
                self.stream_buffer.clear_unescaped();
            }
            _ => {}
        }
        self.handler.handle_event(Event::EndDocument).map_err(PushParseError::Handler)
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
                    ParserState::ParsingString
                }
                ujson::Event::Begin(ujson::EventToken::Key) => {
                    should_append = false;
                    ParserState::ParsingKey
                }
                ujson::Event::Begin(ujson::EventToken::Number) => ParserState::ParsingNumber,
                ujson::Event::ObjectStart => {
                    self.handler.handle_event(Event::StartObject).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ObjectEnd => {
                    self.handler.handle_event(Event::EndObject).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ArrayStart => {
                    self.handler.handle_event(Event::StartArray).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::ArrayEnd => {
                    self.handler.handle_event(Event::EndArray).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::True) => {
                    self.handler.handle_event(Event::Bool(true)).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::False) => {
                    self.handler.handle_event(Event::Bool(false)).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                ujson::Event::End(ujson::EventToken::Null) => {
                    self.handler.handle_event(Event::Null).map_err(PushParseError::Handler)?;
                    ParserState::Idle
                }
                _ => ParserState::Idle,
            },
            ParserState::ParsingString => {
                should_append = false;
                match event {
                    ujson::Event::End(ujson::EventToken::String) => {
                        let s = self.stream_buffer.get_unescaped_slice()?;
                        let s_str = core::str::from_utf8(s)?;
                        self.handler.handle_event(Event::String(String::Unescaped(s_str))).map_err(PushParseError::Handler)?;
                        self.stream_buffer.clear_unescaped();
                        ParserState::Idle
                    }
                    _ => self.append_escape_content(event, pos, data)?,
                }
            }
            ParserState::ParsingKey => {
                should_append = false;
                match event {
                    ujson::Event::End(ujson::EventToken::Key) => {
                        let s = self.stream_buffer.get_unescaped_slice()?;
                        let s_str = core::str::from_utf8(s)?;
                        self.handler.handle_event(Event::Key(String::Unescaped(s_str))).map_err(PushParseError::Handler)?;
                        self.stream_buffer.clear_unescaped();
                        ParserState::Idle
                    }
                    _ => self.append_escape_content(event, pos, data)?,
                }
            }
            ParserState::ParsingNumber => match event {
                ujson::Event::End(ujson::EventToken::Number) | ujson::Event::End(ujson::EventToken::NumberAndArray) | ujson::Event::End(ujson::EventToken::NumberAndObject) => {
                    let s = self.stream_buffer.get_unescaped_slice()?;
                    let num = JsonNumber::from_slice(s)?;
                    self.handler.handle_event(Event::Number(num)).map_err(PushParseError::Handler)?;
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
                // This marks the beginning of an escape, but we handle the specific escapes below
            }
            ujson::Event::End(ujson::EventToken::EscapeQuote) => self.stream_buffer.append_unescaped_byte(b'"')?,
            ujson::Event::End(ujson::EventToken::EscapeBackslash) => self.stream_buffer.append_unescaped_byte(b'\\')?,
            ujson::Event::End(ujson::EventToken::EscapeSlash) => self.stream_buffer.append_unescaped_byte(b'\\')?,
            ujson::Event::End(ujson::EventToken::EscapeBackspace) => self.stream_buffer.append_unescaped_byte(b'\x08')?,
            ujson::Event::End(ujson::EventToken::EscapeFormFeed) => self.stream_buffer.append_unescaped_byte(b'\x0C')?,
            ujson::Event::End(ujson::EventToken::EscapeNewline) => self.stream_buffer.append_unescaped_byte(b'\n')?,
            ujson::Event::End(ujson::EventToken::EscapeCarriageReturn) => self.stream_buffer.append_unescaped_byte(b'\r')?,
            ujson::Event::End(ujson::EventToken::EscapeTab) => self.stream_buffer.append_unescaped_byte(b'\t')?,
            ujson::Event::End(ujson::EventToken::UnicodeEscape) => {
                if pos >= 5 {
                    let hex_slice = &data[pos.saturating_sub(5)..pos + 1];
                    let hex_provider = |_, _| Ok(hex_slice);
                    let (utf8_bytes, _) =
                        process_unicode_escape_sequence(pos, &mut self.unicode_escape_collector, hex_provider)?;
                    if let Some((bytes, len)) = utf8_bytes {
                        for &byte in &bytes[..len] {
                            self.stream_buffer.append_unescaped_byte(byte)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(self.state)
    }
}

/// An error that can occur during push-based parsing.
#[derive(Debug)]
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
) -> impl FnMut(ujson::Event, usize) + '_ {
    |event, pos| {
        for evt in event_storage.iter_mut() {
            if evt.is_none() {
                *evt = Some((event, pos));
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
