1. **Start with Shared Utility Functions and Enums**

   Begin by extracting small, self-contained pieces that are identical or nearly identical between the two parsers. This avoids borrow checker issues since these are standalone.

   - Move the `EventResult` enum from `slice_parser.rs` to `shared.rs`. It's already used in `SliceParser` for processing tokenizer events, but `StreamParser` has inline logic that can be refactored to use it later.
   - The `handle_simple_escape_token` method in `slice_parser.rs` and `handle_simple_escape` in `stream_parser.rs` are similar. Extract a shared function in `shared.rs`:
     ```
     pub fn process_simple_escape(escape_token: &EventToken, unicode_collector: &mut UnicodeEscapeCollector) -> Result<u8, ParseError> {
         unicode_collector.reset_all();
         EscapeProcessor::process_escape_token(escape_token)
     }
     ```
     Then, in both parsers, call this and then delegate to their specific escape handler (e.g., `copy_on_escape.handle_escape` or `append_byte_to_escape_buffer`).
   - The Unicode escape logic (`process_unicode_escape_with_collector`) is almost identical. Move the core logic to a shared function in `shared.rs` that returns the `utf8_bytes_opt` and `escape_start_pos`, then let each parser handle the appending/copying in their own way. This reduces duplication while keeping buffer-specific code separate.

   This step is low-risk and can be done immediately without changing loops.

2. **Abstract the Buffer Interface**

   Both parsers rely on buffer-like structures (`SliceInputBuffer` and `StreamBuffer`) with common operations (e.g., `current_position`, `is_empty`, `get_string_slice`/ `slice`, etc.). Define a trait in `shared.rs` for the common interface:

   ```
   pub trait ParserBuffer {
       fn is_empty(&self) -> bool;
       fn current_position(&self) -> usize;
       fn get_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError>; // Unified for string/number slicing
       // Add other common methods as needed, e.g., is_past_end for slice-specific
   }
   ```

   - Implement this for `SliceInputBuffer` and `StreamBuffer`.
   - Refactor number parsing (`parse_number_from_buffer` in `slice_parser` and `extract_number_from_state` in `stream_parser`) to use this trait. Update `number_parser::parse_number_event` to take `&impl ParserBuffer`.
   - For streaming-specific methods (e.g., `advance`, `current_byte`), keep them separate for now.

   This sets the foundation for generically handling content extraction without touching the loops yet.

3. **Refactor Escape Handling to a Common Structure**

   The escape processing is similar but differs in timing (range-based in `SliceParser` vs per-byte in `StreamParser`). To consolidate gradually:

   - First, refactor `StreamParser` to use range-based literal copying (like `SliceParser`) instead of per-byte appends after the first escape. This aligns the logic:
     - Add a `last_pos: usize` field to `ParserState` in `shared.rs` (initialize to 0).
     - In `stream_parser.rs`, on `Begin(EscapeSequence)`, calculate escape_start = self.stream_buffer.current_position() -1 (position of \ ).
     - Append the literal range from last_pos to escape_start using `stream_buffer.append_unescaped_slice(stream_buffer.get_string_slice(last_pos, escape_start)?)`.
     - On handle_simple_escape or process_unicode_escape_with_collector, update last_pos to escape_start + escape_len (2 for simple, 6 for unicode).
     - On extract_string/key, append the final literal range from last_pos to content_end.
     - Remove the per-byte `handle_byte_accumulation`. This makes `StreamParser` more efficient for long literals and aligns it with `SliceParser`.
     - For surrogate pairs, use the same `if had_pending_high_surrogate { handle(escape_start -6, &[]); handle(escape_start, utf8) }` logic, adapting `append_unescaped_slice`.

   - Once aligned, extract a common `EscapeHandler` trait in `shared.rs`:
     ```
     pub trait EscapeHandler<'a, 'b> {
         fn begin(&mut self, start: usize);
         fn handle_simple_escape(&mut self, escape_start: usize, unescaped: u8) -> Result<(), ParseError>;
         fn handle_unicode_escape(&mut self, escape_start: usize, utf8: &[u8]) -> Result<(), ParseError>;
         fn end(&mut self, end: usize) -> Result<String<'a, 'b>, ParseError>;
     }
     ```
     - Implement `CopyOnEscape` as before for slice.
     - For stream, implement it using `StreamBuffer`'s unescaped methods (e.g., append range slices).
     - Update both parsers to use an instance of this trait (e.g., `self.escape_handler: impl EscapeHandler`).

   This step addresses the main duplication in escape logic without changing the loops much.

4. **Unify Event Pulling Logic**

   The `pull_tokenizer_events` (slice) and the inner loop in `next_event_impl` (stream) for pulling bytes are different due to streaming. Abstract the byte-pulling:

   - Define a trait for input pulling in `shared.rs`:
     ```
     pub trait ByteProvider {
         fn next_byte(&mut self) -> Result<Option<u8>, ParseError>;
         fn finish(&self) -> bool; // True if no more data possible
     }
     ```
     - For slice: `next_byte` = if !buffer.is_empty() { Some(buffer.consume_byte()?) } else { None }
     - For stream: `next_byte` = if buffer.is_empty() { self.fill_buffer_from_reader()?; if buffer.is_empty() { None } else { Some(buffer.current_byte()?) } }; then advance if Some.

   - Refactor both parsers' event-pulling to use this:
     while !self.have_events() {
       if let Some(byte) = self.byte_provider.next_byte()? {
         self.tokenizer.parse_chunk(&[byte], &mut callback);
       } else {
         self.tokenizer.finish(&mut callback);
       }
     }

   For stream, remove `handle_byte_accumulation` (after step 3), as range-based escape handles it.

5. **Consolidate the Core Processing Loop**

   Once the above is done, the `next_event_impl` loops are very similar. Move the common match logic to a shared function in `shared.rs`:

   ```
   pub fn process_event<'a, 'b, H: EscapeHandler<'a, 'b>>(
       event: ujson::Event,
       parser_state: &mut ParserState,
       escape_handler: &mut H,
       buffer: &impl ParserBuffer,
       unicode_collector: &mut UnicodeEscapeCollector,
       from_container_end: &mut bool, // To handle number delimiters
   ) -> EventResult<'a, 'b> {
       match event {
           // Shared match arms for containers, bool, null
           ujson::Event::ObjectStart => EventResult::Complete(Event::StartObject),
           // ... (copy the common arms)
           ujson::Event::Begin(EventToken::String) => {
               parser_state.state = State::String(buffer.current_position());
               escape_handler.begin(buffer.current_position());
               EventResult::Continue
           }
           ujson::Event::End(EventToken::String) => EventResult::ExtractString,
           // For escapes, use the escape_handler
           ujson::Event::Begin(escape_token @ (EventToken::EscapeQuote | ...)) => {
               let unescaped = process_simple_escape(&escape_token, unicode_collector)?;
               escape_handler.handle_simple_escape(buffer.current_position() - escape_token_len, unescaped)?; // Adjust pos as needed
               EventResult::Continue
           }
           // Unicode and other
           // At ExtractString/Key/Number, return the result
           _ => EventResult::Continue,
       }
   }
   ```

   - In each parser's loop, after taking the event, call this function, then match on EventResult to return Complete or extract using buffer/escape_handler.
   - If borrow issues arise (e.g., mut borrows), pass closures for buffer-specific actions (e.g., a closure for calculating escape_start).

6. **Final Generic Parser (Optional Advanced Step)**

   Once the above is stable, consider a generic `JsonParser<B: ParserBuffer + ByteProvider, H: EscapeHandler>` struct that holds the tokenizer, state, buffer, escape_handler, etc. Both `SliceParser` and `StreamParser` can delegate to it or be type aliases. This is the end goal but do it last to avoid big changes.

**General Tips to Avoid Borrow Checker Issues:**
- Use closures to pass buffer-specific logic (e.g., how to get escape_start).
- Test each step with existing examples to ensure no regressions.
- If borrows conflict in loops, split into smaller functions (e.g., one for pulling events, one for processing one event).
- Since changes are gradual, commit after each step.

This plan minimizes risk while progressively reducing duplication.