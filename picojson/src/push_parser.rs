// SPDX-License-Identifier: Apache-2.0

//! A SAX-style, `no_std` JSON push parser.

use crate::event_processor::ParserCore;
use crate::escape_processor::UnicodeEscapeCollector;
use crate::stream_buffer::StreamBuffer;
use crate::{ujson, Event, ParseError, String};

#[derive(Debug, PartialEq, Eq)]
enum State {
    Idle,
    BuildingKey { start: usize },
    BuildingString { start: usize },
}

#[derive(Debug, PartialEq, Eq)]
enum DeferredProcessing {
    None,
    Key,
    String,
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
    stream_buffer: StreamBuffer<'b>,
    state: State,
    unicode_escape_collector: UnicodeEscapeCollector,
    deferred_processing: DeferredProcessing,
    _phantom: core::marker::PhantomData<(&'a (), E)>,
}

impl<'a, 'b, H, C, E> PushParser<'a, 'b, H, C, E>
where
    H: PushParserHandler<'a, 'b, E>,
    C: crate::BitStackConfig,
{
    /// Creates a new `PushParser`.
    pub fn new(handler: H, buffer: &'b mut [u8]) -> Self {
        Self {
            core: ParserCore::new(),
            handler,
            stream_buffer: StreamBuffer::new(buffer),
            state: State::Idle,
            unicode_escape_collector: UnicodeEscapeCollector::new(),
            deferred_processing: DeferredProcessing::None,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Finishes parsing and flushes any remaining events.
    pub fn finish(&mut self) -> Result<(), PushParseError<E>> {
        let mut error: Option<PushParseError<E>> = None;
        let mut callback = |event, _pos| {
            if error.is_some() {
                return;
            }
            let result = match event {
                ujson::Event::ObjectStart => self.handler.handle_event(Event::StartObject),
                ujson::Event::ObjectEnd => self.handler.handle_event(Event::EndObject),
                ujson::Event::ArrayStart => self.handler.handle_event(Event::StartArray),
                ujson::Event::ArrayEnd => self.handler.handle_event(Event::EndArray),
                ujson::Event::End(ujson::EventToken::True) => {
                    self.handler.handle_event(Event::Bool(true))
                }
                ujson::Event::End(ujson::EventToken::False) => {
                    self.handler.handle_event(Event::Bool(false))
                }
                ujson::Event::End(ujson::EventToken::Null) => {
                    self.handler.handle_event(Event::Null)
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                error = Some(PushParseError::Handler(e));
            }
        };

        self.core
            .tokenizer
            .finish(&mut callback)
            .map_err(|e| PushParseError::Parse(e.into()))?;

        if let Some(e) = error {
            Err(e)
        } else {
            // Signal end of document
            self.handler.handle_event(Event::EndDocument)
                .map_err(PushParseError::Handler)
        }
    }

    /// Destroys the parser and returns the handler.
    pub fn destroy(self) -> H {
        self.handler
    }

    /// Writes a slice of bytes to the parser.
    pub fn write(&mut self, data: &'a [u8]) -> Result<(), PushParseError<E>> {
        // Apply any queued unescaped buffer clear from previous processing
        self.stream_buffer.apply_unescaped_reset_if_queued();
        
        // Phase 2: Fill the StreamBuffer for escape processing
        if let Some(fill_slice) = self.stream_buffer.get_fill_slice() {
            let copy_len = data.len().min(fill_slice.len());
            if copy_len > 0 {
                fill_slice[..copy_len].copy_from_slice(&data[..copy_len]);
                // Update data_end to reflect new data
                self.stream_buffer.mark_filled(copy_len).map_err(|_| PushParseError::Parse(crate::ParseError::ScratchBufferFull))?;
            }
        }

        // Phase 2: Collect escape events for later processing to avoid borrowing conflicts
        let mut escape_events: [Option<(ujson::Event, usize)>; 8] = core::array::from_fn(|_| None);
        let mut escape_event_count = 0;
        
        let mut error: Option<PushParseError<E>> = None;
        let mut current_pos = 0;
        let mut callback = |event, pos| {
            if error.is_some() {
                return;
            }
            current_pos = pos;
            
            // Collect escape events for later processing
            match event {
                ujson::Event::Begin(ujson::EventToken::EscapeSequence) |
                ujson::Event::Begin(ujson::EventToken::UnicodeEscape) |
                ujson::Event::End(ujson::EventToken::UnicodeEscape) |
                ujson::Event::End(ujson::EventToken::EscapeQuote) |
                ujson::Event::End(ujson::EventToken::EscapeBackslash) |
                ujson::Event::End(ujson::EventToken::EscapeSlash) |
                ujson::Event::End(ujson::EventToken::EscapeBackspace) |
                ujson::Event::End(ujson::EventToken::EscapeFormFeed) |
                ujson::Event::End(ujson::EventToken::EscapeNewline) |
                ujson::Event::End(ujson::EventToken::EscapeCarriageReturn) |
                ujson::Event::End(ujson::EventToken::EscapeTab) => {
                    if escape_event_count < escape_events.len() {
                        escape_events[escape_event_count] = Some((event.clone(), pos));
                        escape_event_count += 1;
                    }
                }
                _ => {} // Process other events normally
            }
            
            let result = match event {
                // Container events
                ujson::Event::ObjectStart => self.handler.handle_event(Event::StartObject),
                ujson::Event::ObjectEnd => self.handler.handle_event(Event::EndObject),
                ujson::Event::ArrayStart => self.handler.handle_event(Event::StartArray),
                ujson::Event::ArrayEnd => self.handler.handle_event(Event::EndArray),
                
                // Primitive values
                ujson::Event::End(ujson::EventToken::True) => {
                    self.handler.handle_event(Event::Bool(true))
                }
                ujson::Event::End(ujson::EventToken::False) => {
                    self.handler.handle_event(Event::Bool(false))
                }
                ujson::Event::End(ujson::EventToken::Null) => {
                    self.handler.handle_event(Event::Null)
                }
                
                // Key handling
                ujson::Event::Begin(ujson::EventToken::Key) => {
                    self.state = State::BuildingKey { start: pos + 1 };
                    Ok(())
                }
                ujson::Event::End(ujson::EventToken::Key) => {
                    if let State::BuildingKey { start } = self.state {
                        self.state = State::Idle;
                        // Phase 3: Check if we have unescaped content, otherwise use zero-copy
                        if self.stream_buffer.has_unescaped_content() {
                            // Defer processing to after callback to avoid lifetime issues
                            self.deferred_processing = DeferredProcessing::Key;
                            Ok(())
                        } else {
                            // No escapes - use input data (zero-copy)
                            let key_bytes = &data[start..pos];
                            if let Ok(key_str) = crate::shared::from_utf8(key_bytes) {
                                self.handler.handle_event(Event::Key(String::Borrowed(key_str)))
                            } else {
                                Ok(()) // Invalid UTF-8, skip
                            }
                        }
                    } else {
                        Ok(()) // Should not happen
                    }
                }
                
                // String value handling
                ujson::Event::Begin(ujson::EventToken::String) => {
                    self.state = State::BuildingString { start: pos + 1 };
                    Ok(())
                }
                ujson::Event::End(ujson::EventToken::String) => {
                    if let State::BuildingString { start } = self.state {
                        self.state = State::Idle;
                        // Phase 3: Check if we have unescaped content, otherwise use zero-copy
                        if self.stream_buffer.has_unescaped_content() {
                            // Defer processing to after callback to avoid lifetime issues
                            self.deferred_processing = DeferredProcessing::String;
                            Ok(())
                        } else {
                            // No escapes - use input data (zero-copy)
                            let string_bytes = &data[start..pos];
                            if let Ok(string_str) = crate::shared::from_utf8(string_bytes) {
                                self.handler.handle_event(Event::String(String::Borrowed(string_str)))
                            } else {
                                Ok(()) // Invalid UTF-8, skip
                            }
                        }
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

        // Process collected escape events after tokenizer completes
        if let Some(e) = error {
            return Err(e);
        }
        
        // Process escape events that were collected during tokenization
        for i in 0..escape_event_count {
            if let Some((event, pos)) = &escape_events[i] {
                match self.process_escape_event(event.clone(), *pos, data) {
                    Ok(()) => {}
                    Err(e) => return Err(e),
                }
            }
        }

        // Handle deferred string/key processing now that escapes are processed
        match self.deferred_processing {
            DeferredProcessing::Key => {
                self.deferred_processing = DeferredProcessing::None;
                self.process_deferred_key()?;
            }
            DeferredProcessing::String => {
                self.deferred_processing = DeferredProcessing::None;
                self.process_deferred_string()?;
            }
            DeferredProcessing::None => {}
        }

        Ok(())
    }

    /// Process a deferred key with unescaped content
    fn process_deferred_key(&mut self) -> Result<(), PushParseError<E>> {
        if self.stream_buffer.has_unescaped_content() {
            // We still have the fundamental lifetime issue - the deferred clear pattern
            // doesn't solve the core problem that String::Unescaped needs to borrow 
            // from the buffer for lifetime 'b, but we need to modify the buffer
            
            // For now, queue the clear and skip the event until we can resolve the architecture
            self.stream_buffer.queue_unescaped_reset();
            Ok(())
        } else {
            Ok(()) // No unescaped content, nothing to do
        }
    }

    /// Process a deferred string with unescaped content
    fn process_deferred_string(&mut self) -> Result<(), PushParseError<E>> {
        if self.stream_buffer.has_unescaped_content() {
            // The deferred clear pattern is the correct approach, but we have a fundamental
            // lifetime issue with PushParser's current architecture:
            // - The Event<'a, 'b> expects String::Unescaped(&'b str) to live for lifetime 'b
            // - But 'b is tied to the entire PushParser instance, not just the handler call
            // - This prevents us from clearing the buffer after the handler returns
            
            // The solution would require restructuring the lifetimes or using a different
            // approach like copying the unescaped content to a temporary buffer
            
            // For now, queue the clear and skip the event
            self.stream_buffer.queue_unescaped_reset();
            Ok(())
        } else {
            Ok(()) // No unescaped content, nothing to do
        }
    }

    /// Process a single escape event that was collected during tokenization
    fn process_escape_event(&mut self, event: ujson::Event, pos: usize, data: &[u8]) -> Result<(), PushParseError<E>> {
        match event {
            ujson::Event::Begin(ujson::EventToken::EscapeSequence) => {
                // Start escape processing if we're in a string or key
                match self.state {
                    State::BuildingKey { .. } | State::BuildingString { .. } => {
                        self.start_escape_processing(data, pos)?;
                    }
                    _ => {} // Ignore escapes outside strings/keys
                }
                Ok(())
            }
            ujson::Event::Begin(ujson::EventToken::UnicodeEscape) => {
                // Unicode escape sequence start (\uXXXX)
                match self.state {
                    State::BuildingKey { .. } | State::BuildingString { .. } => {
                        self.unicode_escape_collector.reset();
                    }
                    _ => {} // Ignore escapes outside strings/keys
                }
                Ok(())
            }
            ujson::Event::End(ujson::EventToken::UnicodeEscape) => {
                // Unicode escape sequence end - process collected hex digits
                match self.state {
                    State::BuildingKey { .. } | State::BuildingString { .. } => {
                        self.process_unicode_escape()?;
                    }
                    _ => {} // Ignore escapes outside strings/keys  
                }
                Ok(())
            }
            
            // Simple escape sequences (like \n, \", \\, etc.)
            ujson::Event::End(ujson::EventToken::EscapeQuote) => {
                self.process_simple_escape(b'"')
            }
            ujson::Event::End(ujson::EventToken::EscapeBackslash) => {
                self.process_simple_escape(b'\\')
            }
            ujson::Event::End(ujson::EventToken::EscapeSlash) => {
                self.process_simple_escape(b'/')
            }
            ujson::Event::End(ujson::EventToken::EscapeBackspace) => {
                self.process_simple_escape(0x08)
            }
            ujson::Event::End(ujson::EventToken::EscapeFormFeed) => {
                self.process_simple_escape(0x0C)
            }
            ujson::Event::End(ujson::EventToken::EscapeNewline) => {
                self.process_simple_escape(b'\n')
            }
            ujson::Event::End(ujson::EventToken::EscapeCarriageReturn) => {
                self.process_simple_escape(b'\r')
            }
            ujson::Event::End(ujson::EventToken::EscapeTab) => {
                self.process_simple_escape(b'\t')
            }
            _ => Ok(()) // Should not happen
        }
    }

    /// Start escape processing by switching to buffer-based parsing  
    fn start_escape_processing(&mut self, data: &[u8], escape_pos: usize) -> Result<(), PushParseError<E>> {
        // Phase 2: Copy content up to the escape position to the buffer and start unescaping
        let (content_start, content_end) = match self.state {
            State::BuildingKey { start } => (start, escape_pos),
            State::BuildingString { start } => (start, escape_pos),
            _ => return Ok(()), // Should not happen
        };

        if content_end > content_start {
            // Copy the content before the escape to the buffer  
            let pre_escape_content = &data[content_start..content_end];
            self.stream_buffer.start_unescaping_with_copy(
                pre_escape_content.len(), 
                0, 
                pre_escape_content.len()
            ).map_err(|_| PushParseError::Parse(ParseError::ScratchBufferFull))?;
            
            // Copy the content
            for &byte in pre_escape_content {
                self.stream_buffer.append_unescaped_byte(byte)
                    .map_err(|_| PushParseError::Parse(ParseError::ScratchBufferFull))?;
            }
        } else {
            // No content before escape - just start unescaping mode
            self.stream_buffer.start_unescaping_with_copy(0, 0, 0)
                .map_err(|_| PushParseError::Parse(ParseError::ScratchBufferFull))?;
        }
        
        Ok(())
    }

    /// Process a completed unicode escape sequence
    fn process_unicode_escape(&mut self) -> Result<(), PushParseError<E>> {
        // Phase 3: Process unicode escape using StreamBuffer's hex digit extraction
        let hex_slice_provider = |start, end| {
            // Use the StreamBuffer to get hex digits from the current position
            self.stream_buffer.get_string_slice(start, end).map_err(Into::into)
        };

        // Get current position for hex digit extraction
        let current_pos = self.stream_buffer.current_position();
        
        // Process the unicode escape sequence
        let (utf8_bytes_result, _) = crate::escape_processor::process_unicode_escape_sequence(
            current_pos,
            &mut self.unicode_escape_collector,
            hex_slice_provider,
        ).map_err(|e| PushParseError::Parse(e))?;

        // Handle the UTF-8 bytes if we have them
        if let Some((utf8_bytes, len)) = utf8_bytes_result {
            // Append the resulting bytes to the unescaped buffer
            for &byte in &utf8_bytes[..len] {
                self.stream_buffer
                    .append_unescaped_byte(byte)
                    .map_err(|_| PushParseError::Parse(ParseError::ScratchBufferFull))?;
            }
        }

        Ok(())
    }

    /// Process a simple escape sequence (like \n, \", etc.)
    fn process_simple_escape(&mut self, escaped_byte: u8) -> Result<(), PushParseError<E>> {
        match self.state {
            State::BuildingKey { .. } | State::BuildingString { .. } => {
                // Append the unescaped byte to the buffer
                self.stream_buffer.append_unescaped_byte(escaped_byte)
                    .map_err(|_| PushParseError::Parse(ParseError::ScratchBufferFull))?;
            }
            _ => {} // Ignore escapes outside strings/keys
        }
        Ok(())
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
