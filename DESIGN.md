
---
Foo
# Stax Parser Design Notes

## 1. Goals and Philosophy

This document outlines the design for the `picojson` crate, a high-level, allocation-free JSON pull-parser.

The primary philosophy is to build upon the lean, compact, and low-level `ujson` tokenizer to provide an ergonomic API for consumers.

The core design goals are:
- **Zero Heap Allocations**: The parser must not perform any heap allocations during its operation. All memory will be provided by the caller.
- **Ergonomic API**: The parser should be easy to use and feel idiomatic to a Rust developer.
- **Correctness**: The parser must correctly handle all aspects of the JSON spec, including complex string escapes.
- **Footprint**: As minimal resource footprint as possible. This may come at the cost of execution speed.

## 2. Core API Design: The `PullParser` Trait

Standard `Iterator` trait cannot be implemented because of borrowed return values. So the crate provides a `PullParser` trait instead.

## 3. Memory Management: External Scratch Buffer

To achieve the zero-allocation goal while still handling complex cases like string un-escaping, the parser will not manage its own memory. Instead, the caller can provide a temporary "scratch" buffer during instantiation for operations that require it.

This design was chosen over an internal, fixed-size buffer to avoid complex lifetime issues with the borrow checker and to give the user full control over the memory's size and location (stack, static arena, etc.).

The parser offers constructors that support both zero-copy parsing (when no escapes are present) and parsing with a scratch buffer for handling escaped strings:

```rust
// Conceptual representation of constructors
impl<'a, 'b> SliceParser<'a, 'b> {
    /// Creates a new parser for inputs with no string escapes.
    /// This is a zero-copy, zero-allocation operation.
    pub fn new(input: &'a str) -> Self {
        // ...
    }

    /// Creates a new parser with a scratch buffer for handling escapes.
    /// - `input`: A string slice containing the JSON data.
    /// - `scratch_buffer`: A mutable byte slice for temporary operations.
    pub fn with_buffer(input: &'a str, scratch_buffer: &'b mut [u8]) -> Self {
        // ...
    }
}
```

The 'a lifetime is tied to the input data, while 'b is tied to the scratch buffer.

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

## 6. Core Data Structures

Here is a summary of the core public-facing data structures.

```rust
// The main parser for slices
pub struct SliceParser<'a, 'b> { /* ... private fields ... */ }

// The main parser for streams
pub struct StreamParser<'b, R: Reader> { /* ... private fields ... */ }

// The custom "Cow-like" string type
#[derive(Debug, PartialEq, Eq)]
pub enum String<'a, 'b> {
    Borrowed(&'a str),
    Unescaped(&'b str),
}

// The custom number type for flexible parsing
pub enum JsonNumber<'a, 'b> {
    Borrowed { raw: &'a str, parsed: NumberResult },
    Copied { raw: &'b str, parsed: NumberResult },
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
    Number(JsonNumber<'a, 'b>),
    Bool(bool),
    Null,
}

// The comprehensive error type
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// An error from the underlying tokenizer (e.g., invalid syntax).
    TokenizerError,
    /// The provided scratch buffer was not large enough.
    ScratchBufferFull,
    /// A string slice was not valid UTF-8.
    InvalidUtf8,
    /// A number string could not be parsed as the configured type.
    InvalidNumber,
    // ... other specific error conditions
}
```

## 6. Dealing with non-slice input

To support parsing from sources other than in-memory slices (like files, network sockets, or serial ports), the library provides a `StreamParser`. This parser is built upon a custom `Reader` trait, which is analogous to `std::io::Read` but is available in `no_std` environments.

```rust
pub trait Reader {
    type Error;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}
```

The `StreamParser` takes an implementation of this `Reader` and an internal buffer, processing the input as data becomes available. This allows for efficient parsing of large JSON documents with a small, fixed-size memory footprint.

For convenience, the crate provides `picojson::ChunkReader`, a panic-free `Reader` implementation for parsing from byte slices, which is mostly useful for testing and examples.

## 7. Ergonomics and Returned Values

String and number values returned by the parser are designed for ease of use.

-   `picojson::String` implements `Deref<Target=str>`, so it can be used just like a regular `&str`.
-   `picojson::JsonNumber` provides `as_int()` and `as_f64()` methods for easy conversion, along with `Deref<Target=str>` to access the raw number string. This design supports both zero-copy access to the raw number and convenient conversion to standard numeric types.


```
