// SPDX-License-Identifier: Apache-2.0

//! A SAX-style JSON push parser.
//!
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::event_processor::{ContentExtractor, EscapeTiming, ParserCore};
use crate::push_content_builder::{PushContentBuilder, PushParserHandler};
use crate::shared::{ContentKind, DataSource, State};
use crate::stream_buffer::StreamBufferError;
use crate::{ujson, BitStackConfig, Event, ParseError};

extern crate alloc;

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
pub struct PushParser<'input, 'scratch, H, C>
where
    C: BitStackConfig,
{
    /// Content extractor that handles content extraction and event emission
    extractor: PushContentBuilder<'input, 'scratch>,
    /// The handler that receives events
    handler: H,
    /// Core parser logic shared with other parsers
    core: ParserCore<C::Bucket, C::Counter>,
}

impl<'input, 'scratch, H, C> PushParser<'input, 'scratch, H, C>
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
    pub fn write<E>(&mut self, data: &'input [u8]) -> Result<(), PushParseError<E>>
    where
        H: for<'a, 'b> PushParserHandler<'a, 'b, E>,
        E: From<ParseError>,
    {
        // Apply any queued buffer resets
        self.extractor.apply_unescaped_reset_if_queued();

        // Reset the partial span start flag for this new chunk
        self.core.reset_partial_span_start_flag();

        // Set the input slice for the extractor to iterate over
        self.extractor.set_chunk(data);

        // Use ParserCore to process all bytes in the chunk
        loop {
            match self.core.next_event_impl(
                &mut self.extractor,
                EscapeTiming::OnEnd, // PushParser uses OnEnd timing like StreamParser
                |extractor, byte| {
                    // Selective byte accumulation: only accumulate when scratch buffer is in use
                    // This handles remaining content after escape processing
                    if extractor.has_unescaped_content() {
                        let should_accumulate = match extractor.parser_state() {
                            crate::shared::State::String(_) | crate::shared::State::Key(_) => {
                                // For strings/keys, accumulate regular content (not escapes or delimiters)
                                byte != b'\\' && byte != b'"'
                            }
                            crate::shared::State::Number(_) => {
                                // For numbers, accumulate all content (no escapes in numbers)
                                true
                            }
                            _ => false,
                        };
                        
                        if should_accumulate {
                            extractor.append_unescaped_byte(byte)?;
                        }
                    }
                    Ok(())
                },
            ) {
                Ok(Event::EndDocument) => {
                    // EndDocument during write() means we've consumed all bytes in current chunk
                    break;
                }
                Ok(Event::ContentSpan { kind, start, end, has_escapes }) => {
                    // Handle ContentSpan by extracting content and emitting the appropriate event
                    // For simple case (no escapes), directly extract from input chunk
                    if !has_escapes {
                        let content_slice = self.extractor.get_borrowed_slice(start, end)
                            .map_err(PushParseError::Parse)?;
                        
                        let content_event = match kind {
                            ContentKind::String => {
                                let content_str = core::str::from_utf8(content_slice)?;
                                Event::String(crate::String::Borrowed(content_str))
                            }
                            ContentKind::Key => {
                                let content_str = core::str::from_utf8(content_slice)?;
                                Event::Key(crate::String::Borrowed(content_str))
                            }
                            ContentKind::Number => {
                                let json_number = crate::JsonNumber::from_slice(content_slice)?;
                                Event::Number(json_number)
                            }
                        };
                        
                        self.handler
                            .handle_event(content_event)
                            .map_err(PushParseError::Handler)?;
                    } else {
                        // For escaped content, fall back to the existing escape processing mechanism
                        // This delegates to the byte_accumulator callback pattern for now
                        // TODO: PLACEHOLDER - this will be replaced in Step 4 with proper PartialContentSpan handling
                        continue;
                    }

                    // Apply any queued buffer resets after the event has been processed
                    self.extractor.apply_unescaped_reset_if_queued();
                }
                Ok(Event::PartialContentSpanStart { kind, start, has_escapes_in_this_chunk }) => {
                    // Handle start of content that spans chunk boundaries
                    self.handle_partial_content_span_start(kind, start, has_escapes_in_this_chunk)?;
                }
                Ok(Event::PartialContentSpanEnd { kind, end, has_escapes_in_this_chunk: _ }) => {
                    // Handle end of content that spans chunk boundaries
                    // Create the content event in place to avoid borrowing issues
                    
                    // Convert absolute position to relative position within current chunk
                    let position_offset = self.extractor.position_offset();
                    let relative_end = end.saturating_sub(position_offset);
                    let chunk_len = self.extractor.current_chunk_len();
                    
                    log::debug!("PartialContentSpanEnd: kind={:?}, absolute_end={}, position_offset={}, relative_end={}, chunk_len={}", 
                               kind, end, position_offset, relative_end, chunk_len);
                    
                    // For content that continues from previous chunk, the content in this chunk
                    // starts at position 0 and ends at relative_end (but we need to exclude quotes)
                    let content_start = 0;
                    let content_end = match kind {
                        ContentKind::String | ContentKind::Key => {
                            // For strings and keys, relative_end points to the closing quote
                            // We want content up to (but not including) the closing quote
                            relative_end
                        }
                        ContentKind::Number => {
                            // For numbers, relative_end points after the last digit
                            relative_end
                        }
                    };
                    
                    log::debug!("PartialContentSpanEnd: extracting slice [{}, {})", content_start, content_end);
                    
                    // Append the final part from this chunk to the scratch buffer
                    // First, get and copy the final slice data
                    let final_slice = self.extractor.get_borrowed_slice(content_start, content_end)
                        .map_err(PushParseError::Parse)?;
                        
                    log::debug!("PartialContentSpanEnd: final_slice = {:?}", 
                               core::str::from_utf8(final_slice).unwrap_or("[invalid utf8]"));
                    
                    // Copy ALL data to local buffer to completely avoid borrowing conflicts
                    let mut final_data = alloc::vec::Vec::new();
                    final_data.extend_from_slice(final_slice);
                    
                    // Now append from local buffer - no more borrowing conflicts
                    for byte in final_data {
                        self.extractor.append_unescaped_byte(byte)
                            .map_err(PushParseError::Parse)?;
                    }
                        
                    // Get the complete content from the scratch buffer and copy it
                    let complete_content = self.extractor.get_unescaped_slice()
                        .map_err(PushParseError::Parse)?;
                    
                    // Copy to owned data to avoid borrowing conflicts
                    let complete_data = alloc::vec::Vec::from(complete_content);
                    
                    // Queue buffer reset before creating the event
                    self.extractor.queue_unescaped_reset();
                        
                    let content_event = match kind {
                        ContentKind::String => {
                            let content_str = core::str::from_utf8(&complete_data)?;
                            Event::String(crate::String::Unescaped(content_str))
                        }
                        ContentKind::Key => {
                            let content_str = core::str::from_utf8(&complete_data)?;
                            Event::Key(crate::String::Unescaped(content_str))
                        }
                        ContentKind::Number => {
                            let json_number = crate::JsonNumber::from_slice(&complete_data)?;
                            Event::Number(json_number)
                        }
                    };
                    
                    self.handler
                        .handle_event(content_event)
                        .map_err(PushParseError::Handler)?;
                    
                    // Reset the extractor's parser state since content processing is complete
                    *self.extractor.parser_state_mut() = crate::shared::State::None;
                }
                Ok(event) => {
                    // Handle all other events normally
                    self.handler
                        .handle_event(event)
                        .map_err(PushParseError::Handler)?;

                    // Apply any queued buffer resets after the event has been processed
                    // This ensures that buffer content from previous tokens doesn't leak into subsequent ones
                    self.extractor.apply_unescaped_reset_if_queued();
                }
                Err(ParseError::EndOfData) => {
                    // No more events available from current chunk
                    break;
                }
                Err(e) => {
                    return Err(PushParseError::Parse(e));
                }
            }
        }

        // Check for chunk boundary condition - if still processing a token when chunk ends
        let extractor_state = self.extractor.parser_state();

        if matches!(
            extractor_state,
            State::String(_) | State::Key(_) | State::Number(_)
        ) {
            // If we haven't already started using the scratch buffer (e.g., due to escapes)
            if !self.extractor.has_unescaped_content() {
                // Copy the partial content from this chunk to scratch buffer before it's lost
                self.extractor.copy_partial_content_to_scratch()?;
            } else {
                // Special case: For Numbers, check if the scratch buffer is actually empty
                // This handles the byte-by-byte case where the flag is stale from previous Key processing
                if matches!(extractor_state, State::Number(_)) {
                    let buffer_slice = self.extractor.get_unescaped_slice().unwrap_or(&[]);
                    let buffer_empty = buffer_slice.is_empty();

                    if buffer_empty {
                        self.extractor.copy_partial_content_to_scratch()?;
                    }
                }
            }
        }

        // Reset input slice
        self.extractor.reset_input();

        // Update position offset for next call
        self.extractor.add_position_offset(data.len());

        Ok(())
    }

    /// Handle the start of content that spans chunk boundaries
    fn handle_partial_content_span_start<E>(
        &mut self, 
        kind: ContentKind, 
        absolute_start: usize, 
        has_escapes_in_this_chunk: bool
    ) -> Result<(), PushParseError<E>> {
        log::debug!("handle_partial_content_span_start: kind={:?}, absolute_start={}, has_escapes={}", 
                   kind, absolute_start, has_escapes_in_this_chunk);
        
        // Convert absolute position to relative position within current chunk
        let position_offset = self.extractor.position_offset();
        let relative_start = absolute_start.saturating_sub(position_offset);
        let chunk_len = self.extractor.current_chunk_len();
        
        log::debug!("handle_partial_content_span_start: position_offset={}, relative_start={}, chunk_len={}", 
                   position_offset, relative_start, chunk_len);
        
        // For now, we'll implement a simple version that copies byte by byte
        // This can be optimized later with bulk copy methods
        
        let content_slice = self.extractor.get_borrowed_slice(relative_start, chunk_len)
            .map_err(PushParseError::Parse)?;
        
        log::debug!("handle_partial_content_span_start: content_slice = {:?}", 
                   core::str::from_utf8(content_slice).unwrap_or("[invalid utf8]"));
            
        // Copy ALL data to local buffer to completely avoid borrowing conflicts
        let content_data = alloc::vec::Vec::from(content_slice);
        
        // Now append from local buffer - no more borrowing conflicts
        for byte in &content_data {
            self.extractor.append_unescaped_byte(*byte)
                .map_err(PushParseError::Parse)?;
        }
        
        log::debug!("handle_partial_content_span_start: copied {} bytes to scratch buffer", content_data.len());
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
        let extractor_state = self.extractor.parser_state();
        log::debug!("finish(): extractor state = {:?}", extractor_state);
        if *extractor_state != State::None {
            log::error!("finish(): extractor still in state {:?}, returning EndOfData error", extractor_state);
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
