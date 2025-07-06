# Tests

###  Stack depth test results on an Atmega2560 simulator

Generated deeply nested JSON parsing stack behavior test.

Test code can be found [in avr_demo](https://github.com/kaidokert/picojson-rs/tree/main/avr_demo), the numbers are as of [version 0.1.3](https://github.com/kaidokert/picojson-rs/releases/tag/v0.1.3) from [test run here](https://github.com/kaidokert/picojson-rs/actions/runs/16097454843).

*   `serde`: The default [`serde-json-core`](https://crates.io/crates/serde-json-core) implementation.
*   `slice-tiny`: [`picojson`](https://crates.io/crates/picojson) SliceParser with its default 32-level nesting limit.
*   `slice-small`: `picojson` SliceParser configured for a 512-level nesting limit.
*   `slice-huge`: `picojson` SliceParser configured for a 2048-level nesting limit.
*   `stream-tiny`: `picojson` StreamParser with its default 32-level nesting limit.
*   `stream-small`: `picojson` StreamParser configured for a 512-level nesting limit.
*   `stream-huge`: `picojson` StreamParser configured for a 2048-level nesting limit.


| Nesting Depth | serde | slice-tiny | slice-small | slice-huge | stream-tiny | stream-small | stream-huge|
|---|---|---|---|---|---|---|---|
| 7 levels | 208 bytes | 192 bytes | 289 bytes | 484 bytes | 188 bytes | 289 bytes | 481 bytes |
| 9 levels | 224 bytes | 192 bytes | 289 bytes | 484 bytes | 188 bytes | 289 bytes | 481 bytes |
| 30 levels | 392 bytes | 192 bytes | 289 bytes | 484 bytes | 188 bytes | 289 bytes | 481 bytes |
| 33 levels | 416 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 62 levels | 648 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 65 levels | 672 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 126 levels | 1160 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 129 levels | 1184 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 254 levels | 2184 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 257 levels | 2208 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 510 levels | 4232 bytes | Clean Fail | 289 bytes | 484 bytes | Clean Fail | 289 bytes | 481 bytes |
| 513 levels | 4256 bytes | Clean Fail | Clean Fail | 484 bytes | Clean Fail | Clean Fail | 481 bytes |
| 1022 levels | Stack Overflow (Binary Output) | Clean Fail | Clean Fail | 484 bytes | Clean Fail | Clean Fail | 481 bytes |
| 1025 levels | Stack Overflow (Binary Output) | Clean Fail | Clean Fail | 484 bytes | Clean Fail | Clean Fail | 481 bytes |

## Binary Size Analysis (cargo-bloat)

| Configuration | Binary Size |
|---|---|
| serde | 7.4 KB |
| picojson-slice | 7.8 KB |
| picojson-stream | 9.6 KB |

## Summary

picojson's code size is currently larger. This might be due to default 32-bit math operations being compiled in, or generally less optimized code generation. Some const-optimizations could help reduce the size.

Stack usage follows the expected patterns: serde recurses and with more nested documents consumes more stack. picojson is designed to use constant allocation for
its configured max doc depth.
