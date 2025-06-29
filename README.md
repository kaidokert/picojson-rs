# picojson-rs

A minimal Rust JSON parser for resource constrained environments.

- Pull style parsers from byte slices or Reader interface - e.g streaming
- No recursion
- No allocations
- No required dependencies
- User-configured max parsing tree depth
- Configuration of int32 / int64 support
- Configuration and disabling of float support
- no_std by default
