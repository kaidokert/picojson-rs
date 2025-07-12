# Practical Refactoring Plan for picojson-rs Parser Consolidation

## Context: What We're Actually Dealing With

After analyzing the codebase, here's the reality:

- **SliceParser**: 925 lines, clean and simple
- **StreamParser**: 1,851 lines, complex buffer management
- **~60% duplication** in event processing logic
- **Well-designed shared components** already exist (EscapeProcessor, UnicodeEscapeCollector)
- **High-quality codebase** with no unsafe code and good test coverage

## The Real Problem

The main duplication is in the **event processing loops** - both parsers have nearly identical logic for handling tokenizer events, but they're embedded in different contexts (slice vs streaming). The original REF.md plan is ambitious but risks over-engineering a working system.

## Pragmatic 3-Phase Approach

### Phase 1: Extract Common Event Processing (Low Risk, High Value)

**Goal**: Eliminate ~60% of duplication with minimal architectural changes.

**What to do**:
1. Create `src/event_processor.rs` with a shared event processing function:
   ```rust
   pub fn process_tokenizer_event<B: BufferLike>(
       event: ujson::Event,
       state: &mut ParserState,
       buffer: &B,
       unicode_collector: &mut UnicodeEscapeCollector,
   ) -> EventResult {
       match event {
           ujson::Event::ObjectStart => EventResult::Complete(Event::StartObject),
           ujson::Event::ArrayStart => EventResult::Complete(Event::StartArray),
           // ... all the identical match arms
       }
   }
   ```

2. Define minimal `BufferLike` trait with just what's needed:
   ```rust
   pub trait BufferLike {
       fn current_position(&self) -> usize;
       fn get_slice(&self, start: usize, end: usize) -> Result<&[u8], ParseError>;
   }
   ```

3. Both parsers call this shared function, then handle the `EventResult` in their specific way.

**Why this works**: The event matching logic is identical - only the buffer access patterns differ.

### Phase 2: Standardize Position Management (Medium Risk, Medium Value)

**Goal**: Unify how both parsers track positions for error reporting and content extraction.

**What to do**:
1. Extend the existing `ContentRange` utilities in `shared.rs`
2. Both parsers use identical position calculation methods
3. Standardize error reporting with consistent position information

**Why this helps**: Better error messages and less code to maintain.

### Phase 3: Optional Buffer Abstraction (High Risk, Evaluate Later)

**Goal**: Share more buffer operations, but only if Phase 1-2 prove successful.

**What to consider**:
- SliceParser is simple and fast - don't complicate it unnecessarily
- StreamParser's buffer management is complex for good reasons
- Only proceed if there's clear evidence of maintenance burden

## What NOT to Do (Lessons from the Original Plan)

1. **Don't create a mega-generic parser** - the two parsers serve different use cases
2. **Don't over-abstract escape handling** - it's already well-designed
3. **Don't touch the buffer implementations** unless absolutely necessary
4. **Don't change the public APIs** - they're working well

## Implementation Strategy

### Step 1: Test the Waters
1. Create a simple shared function for just container events (StartObject, EndObject, etc.)
2. See how the borrow checker reacts
3. Verify no performance regression with existing benchmarks

### Step 2: Gradual Expansion
1. Add more event types to the shared processor
2. Handle the trickier cases (strings, numbers, escapes) one by one
3. Keep escape handling delegation to existing mechanisms

### Step 3: Measurement and Validation
1. Run full test suite after each change
2. Check that conformance tests still pass
3. Verify no performance regressions
4. Ensure code size doesn't increase significantly

## Success Metrics

- **Reduce duplication** from ~60% to <20% in event processing
- **Maintain performance** (no more than 5% regression)
- **Keep complexity manageable** (don't make SliceParser harder to understand)
- **Preserve safety** (no unsafe code, ever)

## Risk Mitigation

1. **Work in feature branches** - each phase is a separate PR
2. **Comprehensive testing** - run conformance tests after each change
3. **Rollback plan** - each change should be easily reversible
4. **Size budgets** - don't let any single change exceed 200 lines
5. **Performance gates** - benchmark critical paths

## Why This Plan is Better

1. **Realistic scope** - targets the actual duplication, not theoretical perfection
2. **Preserves working code** - both parsers continue to work as designed
3. **Incremental progress** - each phase delivers value independently
4. **Low risk** - changes are isolated and testable
5. **Respects the codebase** - works with existing patterns, not against them

## Success Criteria for Each Phase

**Phase 1 Complete When**:
- Both parsers use shared event processing for container/primitive events
- All tests pass
- No performance regression
- Code duplication reduced by 40%+

**Phase 2 Complete When**:
- Position management is unified
- Error messages are consistent
- Code duplication reduced by 60%+

**Phase 3 Decision Point**:
- Evaluate if further abstraction is worth the complexity
- Only proceed if maintenance burden is clearly reduced

This plan respects the quality of the existing codebase while targeting the real problem: duplicated event processing logic.