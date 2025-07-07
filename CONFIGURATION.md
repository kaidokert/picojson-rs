# Configurable Number Handling

The JSON parser provides comprehensive configurability for number handling, making it suitable for both full-featured and embedded environments.

## Feature Flags

### Integer Width
Choose the integer type to avoid pulling in unnecessary math routines:

- **`int64`** (default): Use `i64` for full range integer support
- **`int32`**: Use `i32` for embedded targets (no 64-bit math routines)

### Float Support
Control float parsing behavior:

- **`float`**: Enable full f64 parsing support
- **No float feature**: Disable float parsing (multiple behavior options available)

### Float Behavior (when `float` feature is disabled)
Choose what happens when floats are encountered:

- **Default**: Return `FloatDisabled` with raw string preserved for manual parsing
- **`float-error`**: Fail parsing when floats are encountered (embedded fail-fast)
- **`float-truncate`**: Truncate simple decimals to integers (1.7 → 1, errors on scientific notation)
- **`float-skip`**: Skip float values during parsing (continue with next token)

## Configuration Examples

### Full Featured (Default)
```toml
[dependencies]
picojson = { path = "../picojson", features = ["int64", "float"] }
```
- 64-bit integers, full float support
- Best for desktop/server applications

### Embedded Friendly
```toml
[dependencies]
picojson = { path = "../picojson", features = ["int32", "float-error"] }
```
- 32-bit integers (no 64-bit math)
- Error on floats (fail fast)
- Minimal code size for embedded systems

### Embedded with Float Tolerance
```toml
[dependencies]
picojson = { path = "../picojson", features = ["int32", "float-truncate"] }
```
- 32-bit integers
- Truncate simple decimals to integers (1.7 → 1)
- Error on scientific notation (avoids float math)

### Legacy Float Disabled
```toml
[dependencies]
picojson = { path = "../picojson", features = ["int64"] }
```
- 64-bit integers
- Floats return `FloatDisabled` with raw string preserved
- Manual parsing available via `JsonNumber::parse()`

## API Usage

All configurations preserve the exact raw string of a number, while providing different parsed representations through the `JsonNumber` enum.

```rust
use picojson::{Event, JsonNumber};

// In your parsing loop:
match event {
    Event::Number(num) => {
        // The raw string is always available for full precision.
        println!("Raw number: {}", num.as_str());

        // Match on the JsonNumber enum to handle different parsing outcomes.
        match num {
            JsonNumber::Integer(i) => {
                println!("Parsed as integer: {}", i);
            }
            JsonNumber::Float(f) => {
                // This variant is only available if the "float" feature is enabled.
                println!("Parsed as float: {}", f);
            }
            JsonNumber::IntegerOverflow => {
                println!("Integer overflow! Raw value: {}", num.as_str());
            }
            JsonNumber::FloatDisabled => {
                // This variant is used when the "float" feature is disabled
                // and no other float handling (like truncate or error) is active.
                println!("Float parsing is disabled. Raw value: {}", num.as_str());
                // You can still manually parse if needed:
                // let manual: f64 = num.as_str().parse().unwrap_or(0.0);
            }
            JsonNumber::FloatTruncated(i) => {
                // This variant is used with the "float-truncate" feature.
                println!("Float was truncated to integer: {}", i);
            }
        }

        // Convenience methods are also available.
        if let Some(int_val) = num.as_int() {
            println!("Successfully read as integer: {}", int_val);
        }
        if let Some(float_val) = num.as_f64() {
            // This will only return Some if the "float" feature is enabled.
            println!("Successfully read as float: {}", float_val);
        }
    }
    _ => {}
}
```

## Testing Different Configurations

Run the demo with different configurations. The truncate mode shows both error and success paths:

```bash
# Basic no-float (raw strings preserved)
cargo run --example no_float_demo --no-default-features

# Embedded-friendly with error on floats
cargo run --example no_float_demo --features int32,float-error

# Embedded with float truncation (demonstrates both error and success scenarios)
cargo run --example no_float_demo --features int32,float-truncate

# Full featured
cargo run --example no_float_demo --features int64,float
```

**Note**: The `float-truncate` configuration demonstrates both successful truncation (with simple decimals) and error handling (with scientific notation) by testing two different JSON inputs.

## Scientific Notation Handling

Different configurations handle scientific notation (`1e3`, `2.5e-1`, `1.23e+2`) differently:

| Configuration | Behavior | Rationale |
|---------------|----------|-----------|
| `float` enabled | Full evaluation: `1e3` → 1000.0 | Complete f64 math available |
| `float-error` | Error: `FloatNotAllowed` | Fail fast on any float syntax |
| `float-truncate` | Error: `InvalidNumber` | Avoid float math entirely |
| Default (disabled) | Raw string: `"1e3"` preserved | Manual parsing available |

**Why truncate mode errors on scientific notation?**
Properly evaluating `1e3` to `1000` requires floating-point arithmetic, which defeats the purpose of embedded no-float configurations. The truncate mode is designed for simple cases like `1.7` → `1` where no exponentiation is needed.

## Benefits

- **Zero runtime overhead**: Behavior configured at compile time
- **Exact precision**: Raw strings always preserved
- **Embedded friendly**: Avoid 64-bit math and float routines when not needed
- **Flexible**: Choose the right tradeoffs for your use case
- **no_std compatible**: No heap allocations
- **Fail fast**: Error configurations catch incompatible data early
