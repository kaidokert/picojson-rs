# AVR measurement fixtures

Stack usage and execution time are collected by the shared Rust campaign
runner. From a checkout where `krabi-caliper` is the sibling used by the
Cargo patch, build its host CLI and run the picojson matrix with:

```sh
cargo install --path ../../krabi-caliper --features cli
cargo krabi-caliper run picojson-avr-stack --quick
cargo krabi-caliper run picojson-avr-stack
```

Run these commands in this directory. The quick form exercises all nine parser
configurations at depth 7; the full form exercises the depth matrix declared in
`krabi-caliper.toml`. Reports, raw simulator output, build logs, and retained
ELFs are written under `target/krabi-caliper/picojson-avr-stack/`.

The Rust runner invokes `simavr` itself and enforces the configured deadline,
so Cargo's AVR target configuration deliberately has no Python runner. The
remaining `run_suite.py` is only for the independent `cargo-bloat` and
panic-reference analyses; it no longer owns stack measurement.

The serde, slice-parser, and stream-parser fixtures use the general
`krabi_caliper::Benchmark<3>` API. Each performs one warm-up and three
recorded Timer1 trials, validates parser success, measures stack high-water
through the same benchmark lifecycle, reports the 2,200-byte input as an
application metric, and writes everything through `UfmtReporter`.
There is no fixture-specific sequencing of timer, stack, measurement, and
outcome records; RTT, semihosting, `fmt` UART, and AVR `ufmt` all implement the
same target-side reporter contract.
