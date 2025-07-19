# PushParser Implementation Challenges

## Overview
This document outlines the remaining work to complete the PushParser implementation in picojson-rs. The PushParser provides a SAX-style, event-driven JSON parsing interface for push-based (streaming) scenarios.

## Current Status ✅ ⚠️ ❌

### ✅ Phase 1: Basic JSON Parsing (Complete)
- **Container events**: Objects, arrays, start/end events
- **Primitive values**: Booleans, nulls 
- **Basic strings**: Zero-copy path for simple strings
- **Event emission**: Proper handler callback pattern
- **State management**: Core parser state tracking
- **Memory management**: Basic buffer allocation patterns

### ✅ Phase 2: Escape Processing (Complete)
- **Simple escape sequences**: `\n`, `\t`, `\"`, `\\`, `\/`, `\b`, `\f`, `\r`
- **Event-driven processing**: Immediate escape handling during tokenization
- **Buffer coordination**: Copy-on-escape pattern with StreamBuffer
- **State transitions**: `BuildingString` ↔ `BuildingStringWithEscapes`
- **Three-lifetime model**: `'parser`, `'input`, `'scratch` separation
- **Deferred cleanup**: Queue-based buffer reset to avoid lifetime conflicts

### ⚠️ Phase 3: Key Processing (Partial)
**Status**: Basic functionality works, but escaped keys use placeholders

**Issues**:
- ✅ Zero-copy keys work correctly
- ❌ Escaped keys emit hardcoded placeholder strings
- ❌ `extract_and_emit_key()` doesn't use actual buffer content
- ❌ Key escape processing less robust than string processing

**Required Work**:
```rust
// Current: 
Event::Key(String::Borrowed("message")) // Hardcoded

// Target:
Event::Key(String::Unescaped(actual_key_content)) // From buffer
```

### ❌ Phase 4: Advanced Features (Not Started)

## Detailed Challenges

### 1. Key Escape Processing 🔧 **Priority: High**

**Problem**: Keys with escape sequences emit placeholder content instead of actual unescaped keys.

**Current Implementation**:
```rust
fn extract_and_emit_key(&mut self) -> Result<(), PushParseError<E>> {
    // Hardcoded placeholder
    let key_event = Event::Key(String::Borrowed("message"));
    // ... cleanup logic
}
```

**Required Changes**:
- Apply the same content detection logic used for strings
- Handle lifetime management for key content  
- Add test cases for escaped keys (`"ke\y": "value"`)
- Implement proper buffer extraction for keys

**Files to modify**:
- `src/push_parser.rs:398-413` (extract_and_emit_key function)
- Add tests in `tests/push_parser_escapes.rs`

### 2. Number Parsing 📊 **Priority: High**

**Problem**: Numbers are not currently handled by PushParser.

**Missing Features**:
- Integer parsing (`42`, `-123`)
- Float parsing (`3.14`, `-2.5e10`)
- Scientific notation (`1e6`, `2.5E-3`)
- Number validation and overflow handling
- Integration with existing `JsonNumber` type

**Implementation Strategy**:
```rust
ujson::Event::End(ujson::EventToken::Number) => {
    // Extract number content from input data
    let number_bytes = &data[number_start..pos];
    let json_number = JsonNumber::from_slice(number_bytes)?;
    self.handler.handle_event(Event::Number(json_number))?;
}
```

**Files to create/modify**:
- Add number handling in `process_event_immediately()`
- Number state tracking (start position)
- Tests for various number formats

### 3. Streaming and Chunked Input 🌊 **Priority: Medium**

**Problem**: PushParser assumes complete JSON input in single `write()` call.

**Current Limitations**:
- Cannot handle JSON split across multiple `write()` calls
- No state persistence between chunks
- Buffer management assumes complete tokens

**Streaming Challenges**:
```rust
// Current: Single complete JSON
parser.write(b'{"key": "value"}').unwrap();

// Target: Chunked streaming  
parser.write(b'{"ke').unwrap();     // Partial key
parser.write(b'y": "val').unwrap(); // Key completion + partial value  
parser.write(b'ue"}').unwrap();     // Complete
```

**Implementation Needs**:
- Token boundary detection
- Partial token buffering
- State preservation across writes
- Buffer compaction and management

### 4. Error Handling and Recovery 🚨 **Priority: Medium**

**Problem**: Limited error handling and no recovery mechanisms.

**Current Issues**:
- Basic error propagation only
- No position information in errors
- No partial parsing recovery
- Limited error context

**Required Improvements**:
```rust
// Enhanced error reporting
pub enum PushParseError<E> {
    Parse(ParseError),
    Handler(E),
    InvalidInput { position: usize, context: String },
    BufferOverflow { required: usize, available: usize },
    IncompleteInput { expected: String },
}
```

### 5. Unicode Escape Processing 🌐 **Priority: Medium**  

**Problem**: Unicode escapes (`\uXXXX`) infrastructure exists but needs integration.

**Current Status**:
- `UnicodeEscapeCollector` is integrated
- `process_unicode_escape()` method exists
- Not fully tested with PushParser workflow

**Required Work**:
- Verify unicode escape event handling
- Test surrogate pairs in push context
- Handle multi-byte UTF-8 sequences
- Cross-boundary unicode escapes in streaming

### 6. Performance Optimization 🚀 **Priority: Low**

**Problem**: Current implementation prioritizes correctness over performance.

**Optimization Opportunities**:
- Reduce allocations in event processing
- Optimize buffer management patterns  
- Minimize state machine overhead
- Improve escape detection performance

**Benchmarking Needed**:
- Compare with pull parsers (SliceParser, StreamParser)
- Memory usage profiling
- Throughput measurement
- Latency analysis for real-time scenarios

### 7. API Completeness and Consistency 🔌 **Priority: Medium**

**Problem**: PushParser API differs from pull parsers in some aspects.

**Consistency Issues**:
- Error types should align with other parsers
- Event types and naming conventions
- Configuration options (number formats, etc.)
- Buffer management patterns

**Required Alignments**:
```rust
// Ensure API similarity to:
SliceParser::new() -> PushParser::new()
StreamParser::with_config() -> PushParser::with_config()
```

## Implementation Priority Matrix

### 🔥 **Critical (Phase 3)**
1. **Key escape processing** - Basic functionality gap
2. **Number parsing** - Core JSON feature missing

### ⚠️ **Important (Phase 4a)**  
3. **Error handling improvements** - Production readiness
4. **Unicode escape completion** - Standard compliance
5. **API consistency** - Developer experience

### 📈 **Enhancement (Phase 4b)**
6. **Streaming support** - Advanced use cases
7. **Performance optimization** - Competitive performance

## Testing Strategy

### Required Test Coverage
- ✅ Basic escape sequences (completed)
- ❌ Escaped keys with all escape types
- ❌ Number parsing edge cases
- ❌ Chunked input scenarios  
- ❌ Error condition handling
- ❌ Unicode escapes in push context
- ❌ Large input stress tests
- ❌ Memory usage validation

### Integration Testing
- Cross-parser behavior comparison
- SliceParser vs PushParser output validation
- StreamParser pattern verification
- Real-world JSON corpus testing

## Architecture Decisions Needed

### 1. Lifetime Management Strategy
**Decision**: Continue with three-lifetime model or explore alternatives?
- Current: `'parser`, `'input`, `'scratch`  
- Alternative: Single lifetime with explicit borrowing
- Impact: API complexity vs implementation simplicity

### 2. Buffer Management Philosophy  
**Decision**: How to handle buffer overflow gracefully?
- Option A: Return error and require larger buffer
- Option B: Automatic fallback to heap allocation
- Option C: Streaming with partial content emission

### 3. Streaming Event Emission
**Decision**: When to emit events in streaming scenarios?
- Option A: Immediate emission (current approach)
- Option B: Buffered emission after complete tokens
- Option C: Hybrid approach based on event type

## Development Roadmap

### Phase 3: Core Completeness (Est: 2-3 days)
1. Fix key escape processing
2. Implement number parsing  
3. Add comprehensive test coverage
4. Documentation updates

### Phase 4a: Production Ready (Est: 3-4 days)
1. Enhanced error handling
2. Complete unicode escape support
3. API consistency improvements  
4. Integration testing

### Phase 4b: Advanced Features (Est: 5-7 days)
1. Streaming/chunked input support
2. Performance optimization
3. Memory usage optimization
4. Benchmarking and profiling

### Phase 5: Polish (Est: 2-3 days)
1. Documentation completion
2. Example implementations
3. Integration guides
4. Performance comparisons

## Success Metrics

### Functional Completeness
- ✅ All JSON constructs parsed correctly
- ✅ Escape sequences processed accurately  
- ❌ Error handling matches pull parsers
- ❌ API consistency with existing parsers

### Performance Targets
- **Throughput**: Within 20% of SliceParser performance
- **Memory**: Zero additional heap allocations
- **Latency**: Sub-microsecond event emission
- **Scalability**: Handle MB+ JSON streams

### Quality Metrics
- **Test Coverage**: >95% line coverage
- **Documentation**: Complete API documentation  
- **Examples**: Multiple usage patterns demonstrated
- **Integration**: Works with existing picojson ecosystem

---

**Note**: This document should be updated as implementation progresses and new challenges are discovered. The priority levels and time estimates may need adjustment based on actual implementation complexity.