### no_std Rust pull parser

This crate is designed for no_std environment JSON pull parsing.

Note: For "document" style parsing where all or most of the document is fully
built in memory, please use serde-json with no_std.

However - pull parsing is useful when you need to process large streams within
constrained memory, without building the entire document, and just picking
elements from the dataset that the application needs.

Example usage:
```rust
use stax::{PullParser, Event, String};

// Simple usage (no string escapes expected)
let json = r#"{"switch": 1}"#;
let parser = PullParser::new(json);
for event in parser {
    match event? {
        Event::Key(String::Borrowed(key)) => {
            println!("Key: '{}'", key);
        }
        Event::Number(num) => {
            println!("Number: {}", num.as_str());
        }
        Event::EndDocument => break,
        _ => {}
    }
}

// With escape support
let json = r#"{"message": "Hello\nWorld"}"#;
let mut scratch = [0u8; 1024];
let parser = PullParser::new_with_buffer(json, &mut scratch);
// ... use parser
```

PullParser takes the input stream, and an optional scratch buffer
to write unescaped strings to. If the input string is known not
to contain any escapes ( like newlines or unicodes ) the buffer
is not used and strings are returned as slices over input.

The parser also uses storage for tracking parsing state, one bit for
every nesting level. By default this is a 32-bit int, but can be changed
to arbitrary depth.

This crate has a few configuration features relevant for embedded targets:

 * int64 ( default ) - numbers are returned in int64 values
 * int32 - integers are returned as int32, to avoid 64-bit math on constrained targets, e.g. Cortex-M0
 * float - full float support is included.
 * float-error - Any floating point input will yield an error, to reduce float math dependency
 * float-skip - Float values are skipped.
 * float-truncate - float values are truncated to integers. Scientific notation will generate an error

 Please see examples/no_float_demo.rs

 By default, full float and int64 support is enabled.
