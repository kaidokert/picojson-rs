// SPDX-License-Identifier: Apache-2.0

//! A SAX-style, `no_std` JSON push parser.

use crate::event_processor::ParserCore;
use crate::{ujson, Event, ParseError, String};

#[derive(Debug, PartialEq, Eq)]
enum State {
    Idle,
    BuildingKey { start: usize },
}

/// A trait for handling events from a SAX-style push parser.
pub trait PushParserHandler<'a, 'b, E> {
    /// Handles a single, complete JSON event.
    fn handle_event(&mut self, event: Event<'a, 'b>) -> Result<(), E>;
}

/// A SAX-style, `no_std` JSON push parser.
pub struct PushParser<'a, 'b, H, C, E>
where
    H: PushParserHandler<'a, 'b, E>,
    C: crate::BitStackConfig,
{
    core: ParserCore<C::Bucket, C::Counter>,
    handler: H,
    state: State,
    _phantom: core::marker::PhantomData<(E, &'a (), &'b ())>,
}

impl<'a, 'b, H, C, E> PushParser<'a, 'b, H, C, E>
where
    H: PushParserHandler<'a, 'b, E>,
    C: crate::BitStackConfig,
{
    /// Creates a new `PushParser`.
    pub fn new(handler: H) -> Self {
        Self {
            core: ParserCore::new(),
            handler,
            state: State::Idle,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Destroy
    pub fn destroy(self) -> H {
        self.handler
    }

    /// Writes a slice of bytes to the parser.
    pub fn write(&mut self, data: &'a [u8], _scratch: &mut [u8]) -> Result<(), PushParseError<E>> {
        let mut error: Option<PushParseError<E>> = None;
        let mut current_pos = 0;
        let mut callback = |event, pos| {
            if error.is_some() {
                return;
            }
            current_pos = pos;
            let result = match event {
                ujson::Event::ObjectStart => self.handler.handle_event(Event::StartObject),
                ujson::Event::ObjectEnd => self.handler.handle_event(Event::EndObject),
                ujson::Event::End(ujson::EventToken::True) => {
                    self.handler.handle_event(Event::Bool(true))
                }
                ujson::Event::Begin(ujson::EventToken::Key) => {
                    self.state = State::BuildingKey { start: pos + 1 };
                    Ok(())
                }
                ujson::Event::End(ujson::EventToken::Key) => {
                    if let State::BuildingKey { start } = self.state {
                        let key_bytes = &data[start..pos];
                        let key_str = crate::shared::from_utf8(key_bytes).unwrap(); // Safe for this test
                        self.state = State::Idle;
                        self.handler
                            .handle_event(Event::Key(String::Borrowed(key_str)))
                    } else {
                        Ok(()) // Should not happen
                    }
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                error = Some(PushParseError::Handler(e));
            }
        };

        self.core
            .tokenizer
            .parse_chunk(data, &mut callback)
            .map_err(|e| PushParseError::Parse(e.into()))?;

        if let Some(e) = error {
            Err(e)
        } else {
            Ok(())
        }
    }

    // ... (finish and destroy are the same)
}

/// An error that can occur during push-based parsing.
#[derive(Debug)]
pub enum PushParseError<E> {
    /// An error occurred within the parser itself.
    Parse(ParseError),
    /// An error was returned by the user's handler.
    Handler(E),
}
