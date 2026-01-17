# picojson

[![crate](https://img.shields.io/crates/v/picojson.svg)](https://crates.io/crates/picojson)
[![documentation](https://docs.rs/picojson/badge.svg)](https://docs.rs/picojson/)
[![Build and test](https://github.com/kaidokert/picojson-rs/actions/workflows/build.yaml/badge.svg)](https://github.com/kaidokert/picojson-rs/actions/workflows/build.yaml)
[![Coverage Status](https://coveralls.io/repos/github/kaidokert/picojson-rs/badge.svg?branch=main)](https://coveralls.io/github/kaidokert/picojson-rs?branch=main)

A minimal Rust JSON **pull-parser** for resource-constrained environments.

`picojson` provides a low-level, `no_std` compatible pull-parser that operates without recursion or heap allocations. It is designed for scenarios where memory is limited and direct control over parsing is required, such as embedded systems.

## Features

- **Pull-style Parsing**: Process JSON from byte slices (`SliceParser`) or any source that implements a `Reader` trait (`StreamParser`).
- **Zero Allocations**: The parser does not perform any heap allocations. All memory, including an optional scratch buffer for value copying, is provided by the caller.
- **No Recursion**: The parsing logic is implemented with an iterative loop, ensuring a predictable and flat call stack.
- **`no_std` by Default**: Designed for bare-metal and embedded use cases.
- **Configurable Tree Depth**: The maximum JSON nesting depth is configured by the user at compile time to control stack usage.
- **Configurable Number Handling**: Integer width and float parsing behavior are configurable via feature flags.
- **Unsafe-Free**: The crate contains no `unsafe` code.
- **Panic-Free**: Does not panic.

## Design Philosophy

The core of `picojson` is a minimal, non-recursive tokenizer that uses a bitstack (1 bit per nesting level) to track object/array depth. The parsers build upon this to provide a higher-level event stream.

The design prioritizes a small resource footprint and predictable deterministic execution over speed.

### Value Handling: No copy and Copy-on-Write

- By default, the parser returns values (strings, keys, numbers) as borrowed slices of the original input.

- In certain situations, a value must be copied into a user-provided scratch buffer:
    1.  **String Escapes**: If a string or key contains escape sequences (e.g., `\n`, `\u0041`), its content must be un-escaped into the scratch buffer.
    2.  **Stream Buffering**: When using the `StreamParser`, if a token (like a long number) is split across separate reads from the underlying I/O source, it must be copied into the scratch buffer to be made contiguous.

## Usage

### Parsing from a Slice

Use `SliceParser` when the entire JSON document is in memory. A scratch buffer is required to handle potential string escapes.

```rust
use picojson::{SliceParser, Event, String};

let json = r#"{"message": "Hello\nWorld"}"#;
let mut scratch = [0u8; 32];
let mut parser = SliceParser::with_buffer(json, &mut scratch);

loop {
    match parser.next_event()? {
        Event::Key(key) => { // key is a picojson::String
            println!("Key: {}", key);
        }
        Event::String(value) => {
            println!("Value: {}", value);
        }
        Event::EndDocument => break,
        _ => {}
    }
}
```

### Parsing from a Stream

Use `StreamParser` for parsing from any source that implements the `Reader` trait, such as a file or network socket.

```rust
use picojson::{StreamParser, Event, Reader};

// A simple Reader implementation over a byte slice.
struct MyReader<'a> {
    data: &'a [u8],
    pos: usize,
}
// ... Reader implementation ...

let json_stream = b"{\"id\": 123}";
let mut buffer = [0u8; 1024]; // Buffer for the parser to use.
let mut parser = StreamParser::new(MyReader::new(json_stream), &mut buffer);

// ... event loop ...
```

See the [API docs](https://docs.rs/picojson/) and [examples](https://github.com/kaidokert/picojson-rs/tree/main/picojson/examples) for more details. Some test results can be found on the [project site](https://kaidokert.github.io/picojson-rs/).

## Configuration

The parser's behavior can be customized with feature flags. For example, integer width can be set to `int32` or `int64`, and float handling can be configured to error, truncate, or ignore. For a detailed guide, please see the crate reference documentation.

## Stability

This library is experimental, though the parsers pass conformance tests.

For battle-tested code please use [serde-json-core](https://crates.io/crates/serde-json-core) or various other alternatives.

## License

Apache 2.0; see [`LICENSE`](LICENSE) for details.
