Continue gradually, focusing on one area at a time (test after each). Prioritize aligning escape handling first, as it's the biggest remaining duplicate/hotspot. Aim for 1-2 steps per "iteration" to avoid big refactors.

    Align Escape Handling to Range-Based Processing (High Priority, Aligns StreamParser with SliceParser):
        Refactor StreamParser to copy literal ranges (e.g., between escapes) instead of per-byte appends. This improves efficiency for long strings and makes it more like CopyOnEscape.
            Add a last_literal_pos: usize field to ParserState (in shared.rs) for tracking the start of the current literal span.
            In process_begin_events (shared), on Begin(String/Key), set last_literal_pos = context.current_position().
            On Begin(EscapeSequence) (move this to a shared function if possible), calculate escape_start = context.current_position() - 1 (position of \), then append the literal slice from last_literal_pos to escape_start using stream_buffer.append_unescaped_slice(self.stream_buffer.get_string_slice(last_literal_pos, escape_start)?).
            After handling simple/Unicode escapes (in EscapeHandler::handle_simple_escape_char or process_unicode_escape_with_collector), update last_literal_pos = escape_start + escape_len (2 for simple, 6 for Unicode).
            On ExtractString/Key (in validate_and_extract_string/key), append the final literal from last_literal_pos to content_end.
            Remove handle_byte_accumulation and per-byte appends entirely—this logic becomes event-driven.
            For surrogates, adapt the if had_pending_high_surrogate logic to skip literal appends as needed.
        Benefits: Removes append_byte_to_escape_buffer, aligns with CopyOnEscape's range-based copying, reduces per-byte overhead.
        Test: Use JSON with mixed escapes/literals; ensure compaction doesn't break ranges.
        Time Estimate: 1-2 hours; low borrow risk since it's mostly adding to existing methods.
    Extend EscapeHandler Trait for Full Escape Lifecycle (Builds on Step 1):
        Once ranges are aligned, expand EscapeHandler to cover the full escape process:
        text

    pub trait EscapeHandler {
        // Existing methods...
        fn start_escape(&mut self, content_start: usize, escape_pos: usize) -> Result<(), ParseError>;  // Init unescaping, copy initial literal
        fn append_literal(&mut self, start: usize, end: usize) -> Result<(), ParseError>;  // Shared for ranges
        fn end_escape(&mut self, final_end: usize) -> Result<crate::String<'a, 'b>, ParseError>;  // Final literal + return result
    }
        Implement for CopyOnEscape (map to its handle_escape, handle_unicode_escape, etc.).
        For StreamParser, delegate to StreamBuffer methods (e.g., start_unescaping_with_copy, append_unescaped_slice).
        Move start_escape_processing logic into a shared function that calls start_escape.
        Update loops/extractors to use these (e.g., validate_and_extract_string calls end_escape).
    Benefits: Fully shares escape logic; CopyOnEscape and StreamBuffer become interchangeable via trait.
    Test: Edge cases like escapes at buffer boundaries, surrogates spanning compactions.
    Time Estimate: 1 hour; watch for mut borrows—use closures if needed for buffer access.

Abstract Byte/Input Providing (After Escape Alignment):

    Define a ByteProvider trait (as in the plan):
    text

pub trait ByteProvider {
    fn next_byte(&mut self) -> Result<Option<u8>, ParseError>;
    fn is_finished(&self) -> bool;
}

    Impl for SliceParser: Use buffer.consume_byte(), return None on end.
    Impl for StreamParser: Handle fill/compaction in next_byte (move fill_buffer_from_reader logic here), advance on success.

Refactor event-pulling loops:
text

        while !have_events(&self.parser_state.evts) {
            if let Some(byte) = self.byte_provider.next_byte()? {
                let mut callback = create_tokenizer_callback(&mut self.parser_state.evts);
                self.tokenizer.parse_chunk(&[byte], &mut callback)?;
            } else {
                let mut callback = create_tokenizer_callback(&mut self.parser_state.evts);
                self.tokenizer.finish(&mut callback)?;
                if self.byte_provider.is_finished() && !have_events(&self.parser_state.evts) {
                    return Ok(Event::EndDocument);
                }
            }
        }
        Benefits: Shares the "pull byte → tokenize → finish" loop; reduces stream-specific code.
        Test: Streaming vs. slice EOF handling.
        Time Estimate: 30-45 min; low risk.
    Unify More of the Core Loop (Once Above is Stable):
        Extract a shared process_next_event function in event_processor.rs that takes &mut impl ParserContext + EscapeHandler + ContentExtractor and handles the full take_first_event → process_* → match EventResult.
        Both parsers' next_event_impl become thin wrappers: pull events, then call process_next_event(self).
        If borrows conflict (e.g., multiple mut traits), combine into one uber-trait or pass closures for specifics.
        Benefits: Near-complete loop sharing; parsers differ only in setup/buffer.
        Test: Full parser runs.
        Time Estimate: 1 hour; higher borrow risk—split if needed.
    General Advice:
        Testing: Add unit tests for shared functions (e.g., process_* with mock traits). Use existing examples; focus on escapes/surrogates/compaction.
        Borrow Checker: If issues arise, use temporary vars (e.g., clone event before processing) or split functions further.
        Measure Progress: After step 2, duplication should drop another 30%; after 4, parsers could be ~70% shared.
        Avoid Over-Abstraction: Don't force a single generic parser yet—keep them separate until loops are identical.
        Tools/Verification: If needed, use the code_execution tool to test snippets (e.g., parse a JSON string with escapes).

This keeps momentum while building toward full consolidation. If you hit a snag, share the updated code!
5.2s
wait let me understand what you are saying for step 1.  You understand that the StreamParser is designed to work with potentially very small buffers, right - e.g. it can literally work with whatever the longest unescaped token span is or was in the input. What you are suggesting for this align escape handling isn't at odds with this objective ? 
Thought for 23s

Yes, I understand the StreamParser's design constraints very well—it's built to handle potentially tiny input buffers (e.g., reading from a slow stream like a network socket in small chunks), and the scratch buffer must be at least as large as the longest contiguous token (string, key, or number) to avoid ScratchBufferFull errors during compaction or unescaping. The current implementation already enforces this by erroring out in switch_*_to_escape_mode if any actual content is discarded during compaction (i.e., if discarded_content > 0), meaning it doesn't support recovering partial tokens across compactions if the buffer can't hold the full token span.

My suggestion for step 1 (switching to range-based literal copying in StreamParser) is fully compatible with this and not at odds with the objective. Here's why, broken down carefully:
Compatibility with Small Buffers

    Core Assumption Remains the Same: Both the current per-byte append approach and the proposed range-based one rely on the same key invariant: the scratch buffer must hold the entire token's content at once. If a token exceeds the buffer size, parsing fails with ScratchBufferFull (as in the current switch_*_to_escape_mode logic). The range-based change doesn't weaken this—it just optimizes how literals are copied within the buffer's capacity.
    How Compaction Works (Unchanged): During fill_buffer_from_reader, if the buffer is full and compaction is needed, it shifts from the token's content_start (stored in ParserState). If any content before the current position is discarded and that includes actual token data (i.e., discarded_content > 0), it errors. This protects against losing parts of a token. In the range-based version:
        Literals are only copied on demand (e.g., when an escape is hit or at token end), and at that point, the range (last_literal_pos to escape_start or content_end) is still fully in the buffer—because compaction hasn't discarded it yet (or if it has, we've already errored).
        If compaction happens mid-literal (e.g., buffer fills during a long unescaped span), the compact_start_pos is the token's overall content_start, so last_literal_pos (which is >= content_start) will either be preserved (updated by subtracting offset) or trigger the error if discarded. This mirrors the current behavior.
    No New Risks for Token Spans: In the current code, unescaping starts only on the first escape (start_escape_processing), copying the initial content range at once, then appends per-byte for subsequent literals. If a later literal is very long and causes buffer overflow, compaction checks ensure the whole token fits. In range-based:
        We defer copying literals until an escape or end, but since the buffer holds the input span, and we copy the range atomically (via append_unescaped_slice), it's equivalent—perhaps even safer/more efficient, as we avoid unnecessary per-byte operations.
        For surrogates or interrupted escapes, skipping copies (as in the current if has_pending_high_surrogate() { content_end = content_start }) translates directly: just don't append the range if it's surrogate text.
    Efficiency Gains Without Tradeoffs: Per-byte appends are fine but micro-inefficient for long literals (e.g., a 1KB unescaped string appends 1KB times). Range-based uses slice copies (faster via memcpy-like ops), but since the range is in the buffer (which is already sized for the token), no extra memory or risk. If the buffer is tiny and the token fits, it works; if not, it fails the same way.

Potential Edge Cases (and Why They're Handled)

    Escape at Buffer Boundary: If an escape sequence spans fills/compactions, the tokenizer handles it byte-by-byte, and compaction updates positions. When Begin(EscapeSequence) fires, the range to copy is from last_literal_pos (updated post-compaction) to the current \ position—still valid.
    Long Token Without Escapes: No copying at all until end (like current no-escape case), then one range append at extract. If compaction discards early parts, it errors early (same as now).
    Surrogate Pairs Across Compactions: The had_pending_high_surrogate logic skips including the literal \uD801 text, which in range-based means skipping the range append for that span—straightforward.
    Buffer Smaller Than Token: Both approaches fail with ScratchBufferFull during compaction if content is lost. No change.

If we implement this, StreamParser becomes more like SliceParser (which is already range-based via CopyOnEscape), unlocking further sharing (e.g., unifying EscapeHandler more). It's a net positive for consolidation without compromising the small-buffer goal.

If you're worried about a specific scenario (e.g., a code snippet to test), let me know—I can suggest a test case or refine the step. Otherwise, proceed with it; it's low-risk and aligns perfectly.
5.2s