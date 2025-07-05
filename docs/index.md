# Tests

###  Stack depth test results on an Atmega2560 simulator

Generated deeply nested JSON parsing stack behavior test.

Test code can be found [in avr_demo](https://github.com/kaidokert/picojson-rs/tree/main/avr_demo), the numbers are as of [version 0.1.2](https://github.com/kaidokert/picojson-rs/releases/tag/v0.1.2) from [test run here](https://github.com/kaidokert/picojson-rs/actions/runs/16084368607).

*   `serde`: The default [`serde-json-core`](https://crates.io/crates/serde-json-core) implementation.
*   `picojson-tiny`: [`picojson`](https://crates.io/crates/picojson) with its default 32-level nesting limit.
*   `picojson-small`: `picojson` configured for a 512-level nesting limit.
*   `picojson-huge`: `picojson` configured for a 2048-level nesting limit.


| Nesting Depth | serde | picojson-tiny | picojson-small | picojson-huge|
|---|---|---|---|---|
| 7 levels | 208 bytes | 192 bytes | 289 bytes | 484 bytes |
| 9 levels | 224 bytes | 192 bytes | 289 bytes | 484 bytes |
| 31 levels | 400 bytes | Clean Fail | 289 bytes | 484 bytes |
| 33 levels | 416 bytes | Clean Fail | 289 bytes | 484 bytes |
| 63 levels | 656 bytes | Clean Fail | 289 bytes | 484 bytes |
| 65 levels | 672 bytes | Clean Fail | 289 bytes | 484 bytes |
| 127 levels | 1168 bytes | Clean Fail | 289 bytes | 484 bytes |
| 129 levels | 1184 bytes | Clean Fail | 289 bytes | 484 bytes |
| 255 levels | 2192 bytes | Clean Fail | 289 bytes | 484 bytes |
| 257 levels | 2208 bytes | Clean Fail | 289 bytes | 484 bytes |
| 511 levels | 4240 bytes | Clean Fail | Clean Fail | 484 bytes |
| 513 levels | 4256 bytes | Clean Fail | Clean Fail | 484 bytes |
| 1023 levels | Stack Overflow (Binary Output) | Clean Fail | Clean Fail | 484 bytes |
| 1025 levels | Stack Overflow (Binary Output) | Clean Fail | Clean Fail | 484 bytes |

## Binary Size Analysis (cargo-bloat)

| Configuration | Binary Size |
|---|---|
| serde | 7.4 KB |
| picojson | 7.8 KB |

## Summary

picojson's code size is currently larger. This might be due to default 32-bit math operations being compiled in, or generally less optimized code generation. Some const-optimizations could help reduce the size.

Stack usage follows the expected patterns: serde recurses and with more nested documents consumes more stack. picojson is designed to use constant allocation for
its configured max doc depth.
