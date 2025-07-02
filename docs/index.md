# Tests

###  Stack depth test results on an Atmega2560 simulator

Generated deeply nested JSON parsing stack behavior test.

Test code can be found [in avr_demo](https://github.com/kaidokert/picojson-rs/tree/main/avr_demo), the numbers are as of [version 0.1.2](https://github.com/kaidokert/picojson-rs/releases/tag/v0.1.2) from [test run here](https://github.com/kaidokert/picojson-rs/actions/runs/16017708831).

*   `serde`: The default [`serde-json-core`](https://crates.io/crates/serde-json-core) implementation.
*   `picojson-tiny`: [`picojson`](https://crates.io/crates/picojson) with its default 32-level nesting limit.
*   `picojson-small`: `picojson` configured for a 512-level nesting limit.
*   `picojson-huge`: `picojson` configured for a 2048-level nesting limit.


| Nesting Depth | serde | picojson-tiny | picojson-small | picojson-huge|
|---|---|---|---|---|
| 7 levels | 204 bytes | 202 bytes | 356 bytes | 742 bytes |
| 9 levels | 220 bytes | 202 bytes | 356 bytes | 742 bytes |
| 31 levels | 396 bytes | Clean Fail | 356 bytes | 742 bytes |
| 33 levels | 412 bytes | Clean Fail | 356 bytes | 742 bytes |
| 63 levels | 652 bytes | Clean Fail | 356 bytes | 742 bytes |
| 65 levels | 668 bytes | Clean Fail | 356 bytes | 742 bytes |
| 127 levels | 1164 bytes | Clean Fail | 356 bytes | 742 bytes |
| 129 levels | 1180 bytes | Clean Fail | 356 bytes | 742 bytes |
| 255 levels | 2188 bytes | Clean Fail | 356 bytes | 742 bytes |
| 257 levels | 2204 bytes | Clean Fail | 356 bytes | 742 bytes |
| 511 levels | 4236 bytes | Clean Fail | Clean Fail | 742 bytes |
| 513 levels | 4252 bytes | Clean Fail | Clean Fail | 742 bytes |
| 1023 levels | Stack Overflow | Clean Fail | Clean Fail | 744 bytes |
| 1025 levels | Stack Overflow | Clean Fail | Clean Fail | 744 bytes |

## Binary Size Analysis (cargo-bloat)

| Configuration | Binary Size |
|---|---|
| serde | 9.6 KB |
| picojson | 13.3 KB |

## Summary

picojson's code size is currently larger. This might be due to default 32-bit math operations being compiled in, or generally less optimized code generation. Some const-optimizations could help reduce the size.

Stack usage follows the expected patterns: serde recurses and with more nested documents consumes more stack. picojson is designed to use constant allocation for
its configured max doc depth.
