# Tests

###  Stack depth test results on an Atmega2560 simulator

Generated deeply nested JSON parsing stack behavior test.

*   `serde`: The default [`serde-json-core`](https://crates.io/crates/serde-json-core) implementation.
*   `picojson-tiny`: [`picojson`](https://crates.io/crates/picojson) with its default 32-level nesting limit.
*   `picojson-small`: `picojson` configured for a 512-level nesting limit.
*   `picojson-huge`: `picojson` configured for a 2048-level nesting limit.


| Nesting Depth | serde | picojson-tiny | picojson-small | picojson-huge|
|---|---|---|---|---|
| 7 levels | 204 bytes | 202 bytes | 356 bytes | 742 bytes |
| 9 levels | 220 bytes | 202 bytes | 356 bytes | 742 bytes |
| 31 levels | 396 bytes | Clean Fail | 356 bytes | 742 bytes |
| 63 levels | 652 bytes | Clean Fail | 356 bytes | 742 bytes |
| 127 levels | 1164 bytes | Clean Fail | 356 bytes | 742 bytes |
| 255 levels | 2188 bytes | Clean Fail | 356 bytes | 742 bytes |
| 511 levels | 4236 bytes | Clean Fail | Clean Fail | 742 bytes |
| 513 levels | 4252 bytes | Clean Fail | Clean Fail | 742 bytes |
| 1023 levels | Stack Overflow | Clean Fail | Clean Fail | 744 bytes |
| 1025 levels | Stack Overflow | Clean Fail | Clean Fail | 744 bytes |
