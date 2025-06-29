
---

# Stax Parser Design Notes

## 1. Goals and Philosophy

This document outlines the design for the `stax` crate, a high-level, allocation-free JSON pull-parser.

The primary philosophy is to build upon the lean, compact, and low-level `ujson` tokenizer to provide an ergonomic and highly efficient API for consumers.

The core design goals are:
- **Zero Heap Allocations**: The parser must not perform any heap allocations during its operation. All memory will be provided by the caller.
- **Ergonomic API**: The parser should be easy to use and feel idiomatic to a Rust developer.
- **Correctness**: The parser must correctly handle all aspects of the JSON spec, including complex string escapes.
- **Footprint**: As minimal resource footprint as possible. This may come at the cost of execution speed.

## 2. Core API Design: The `Iterator` Trait

To provide the most idiomatic API, `PullParser` will implement the standard `Iterator` trait. This allows consumers to process JSON events using a simple `for` loop, integrating seamlessly with the rest of the Rust ecosystem.

```rust
// The user-facing API will be clean and simple:
let mut scratch = [0; 1024];
let parser = PullParser::new_with_buffer(json_input, &mut scratch);

for event_result in parser {
    let event = event_result?;
    // ... process event
}
```

The iterator's item will be a `Result` to allow for robust error handling.

```rust
impl<'a, 'b> Iterator for PullParser<'a, 'b> {
    type Item = Result<Event<'a, 'b>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        // ... parsing logic ...
    }
}
```

## 3. Memory Management: External Scratch Buffer

To achieve the zero-allocation goal while still handling complex cases like string un-escaping, the parser will not manage its own memory. Instead, the caller must provide a temporary "scratch" buffer during instantiation.

This design was chosen over an internal, fixed-size buffer to avoid complex lifetime issues with the borrow checker and to give the user full control over the memory's size and location (stack, static arena, etc.).

The parser's constructor will have the following signature:

```rust
impl<'a, 'b> PullParser<'a, 'b> {
    /// Creates a new parser for the given JSON input.
    ///
    /// - `input`: A string slice containing the JSON data to be parsed.
    /// - `scratch_buffer`: A mutable byte slice for temporary operations,
    ///   like string un-escaping.
    pub fn new(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        // ...
    }
}
```

The `'a` lifetime is tied to the input data, while `'b` is tied to the scratch buffer.

## 4. Handling String Values: The `String` Enum

To handle string values efficiently, we will use a custom "Copy-on-Write"-like enum called `String`. This avoids allocations by returning either a view into the original input or a view into the scratch buffer.

```rust
/// Represents a JSON string.
/// 'a is the lifetime of the original input buffer.
/// 'b is the lifetime of the scratch buffer.
#[derive(Debug, PartialEq, Eq)]
pub enum String<'a, 'b> {
    /// A raw slice from the original input, used when no un-escaping is needed.
    Borrowed(&'a str),
    /// A slice from the scratch buffer, used when a string had to be un-escaped.
    Unescaped(&'b str),
}
```

This enum will implement `Deref<Target=str>` so it can be used almost exactly like a standard `&str`, providing excellent ergonomics.

## 5. String Parsing Strategy: "Copy-on-Escape"

To minimize overhead, the parser will adopt a lazy "copy-on-escape" strategy for strings and keys. This optimizes for the most common case where strings do not contain any escape sequences.

The algorithm is as follows:

1.  **Optimistic Fast Path**: When a string token begins, the parser assumes no escapes will be found. It does not perform any copying. If the end of the string is reached without encountering a `\` character, it returns a `String::Borrowed` variant containing a slice of the original input. This is a zero-copy operation.

2.  **Triggered Slow Path**: If a `\` character *is* encountered while scanning the string:
    a. The parser immediately switches to "unescaping mode".
    b. It performs a one-time copy of the string prefix (all characters from the start of the string up to the `\`) into the provided scratch buffer.
    c. It continues processing the rest of the string, un-escaping sequences and writing the processed characters directly into the scratch buffer.
    d. When the end of the string is reached, it returns a `String::Unescaped` variant containing a slice of the now-populated scratch buffer.

This ensures that work is only done when absolutely necessary.

## 6. Final Data Structures

Here is a summary of the core public-facing data structures.

```rust
// The main parser struct
pub struct PullParser<'a, 'b> { /* ... private fields ... */ }

// The custom "Cow-like" string type
#[derive(Debug, PartialEq, Eq)]
pub enum String<'a, 'b> {
    Borrowed(&'a str),
    Unescaped(&'b str),
}

// The events yielded by the iterator
#[derive(Debug, PartialEq)]
pub enum Event<'a, 'b> {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    Key(String<'a, 'b>),
    String(String<'a, 'b>),
    Number(f64), // Assuming f64 for now
    Bool(bool),
    Null,
}

// The comprehensive error type
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// An error bubbled up from the underlying tokenizer.
    Tokenizer(ujson::Error),
    /// The provided scratch buffer was not large enough for an operation.
    ScratchBufferFull,
    /// A string slice was not valid UTF-8.
    InvalidUtf8(core::str::Utf8Error),
    /// A number string could not be parsed.
    InvalidNumber(core::num::ParseFloatError),
    /// The parser entered an unexpected internal state.
    UnexpectedState(&'static str),
}
```

## 6. Dealing with non-slice input

IMPORTANT!!!

More: In addition of taking just slice [u8] as input, we should accept an `impl Reader` of some sort.
So that the input can come no-copy from any source with low buffering

Note std::io has Read trait, but unfortunately that's not available in core::, so probably have to
make our own, and auto-implement it for arrays and slices or for anything that looks like AsRef<[u8]>

## 7. TODO: Working with returned values

String values in stax now have Deref, AsRef and Format support, so using them in default examples
with things like println! is convenient and easy.

Same should be done with Number, but it's a little more tricky to design, given the configuration
variability

## 8. TODO: Add direct defmt support for user API

For any user of the Stax parser with defmt:: enabled, all the formatting should do sensible
default things. Most tricky is number formatting. The objective is to have clean, ergonomic, readable
examples
