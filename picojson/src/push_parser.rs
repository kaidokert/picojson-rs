// SPDX-License-Identifier: Apache-2.0

//! A SAX-style JSON push parser.
//!
//! Clean implementation based on handler_design pattern with proper HRTB lifetime management.

use crate::event_processor::{ContentExtractor, EscapeTiming, ParserCore};
use crate::push_content_builder::{PushContentExtractor, PushParserHandler};
use crate::shared::{DataSource, State};
use crate::stream_buffer::StreamBufferError;
use crate::{ujson, BitStackConfig, Event, ParseError};

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
    extractor: PushContentExtractor<'input, 'scratch>,
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
            extractor: PushContentExtractor::new(buffer),
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

        // Set the input slice for the extractor to iterate over
        self.extractor.set_chunk(data);

        // Use ParserCore to process all bytes in the chunk
        loop {
            match self.core.next_event_impl_with_flags(
                &mut self.extractor,
                EscapeTiming::OnEnd, // PushParser uses OnEnd timing like StreamParser
                |extractor, byte| {
                    // Selective accumulation: let PushContentExtractor decide based on its state
                    // whether this byte should be accumulated or processed directly
                    extractor.handle_byte_accumulation(byte)
                },
                true, // always_accumulate_during_escapes: ensure all hex digits reach the accumulator
            ) {
                Ok(Event::EndDocument) => {
                    // EndDocument during write() means we've consumed all bytes in current chunk
                    break;
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

// Implement From<ParseError> for common error types used in tests
// This needs to be globally accessible for integration tests, not just unit tests
impl From<ParseError> for () {
    fn from(_: ParseError) -> Self {}
}
