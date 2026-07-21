#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use krabi_caliper::Benchmark;
use krabi_caliper::report::{Field, UfmtReporter};
use krabi_caliper::stack::{Avr, LinkerStack, StackConfig};
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Deserialize, Default)]
#[serde(default)]
struct Doc<'a> {
    id: u32,
    test_depth: u32,
    status: &'a str,
}

const JSON_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/test.json"));

#[arduino_hal::entry]
fn main() -> ! {
    let mut dp = arduino_hal::Peripherals::take().unwrap();
    #[cfg(feature = "ufmt")]
    let serial = {
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    let mut reporter = UfmtReporter::new(serial);
    let fields = [Field::token("target", "atmega2560")];
    let benchmark = Benchmark::<3>::new("serde-json-core")
        .warmups(1)
        .fields(&fields);
    let stack = unsafe { LinkerStack::<Avr>::avr_runtime(0x2200) };
    // SAFETY: ATmega2560 SRAM above `_end` is reserved for this single stack.
    unsafe {
        krabi_caliper::avr::run_atmega2560_benchmark(
            &mut dp.TC1,
            Some(15_625),
            &mut reporter,
            &benchmark,
            &stack,
            StackConfig::new(64).sentinel(0xce),
            || {
                let mut scratch = [0u8; 1];
                let parsed: Result<(Doc, _), _> =
                    serde_json_core::from_slice_escaped(JSON_DATA, &mut scratch);
                parsed.is_ok()
            },
        )
    }
    .unwrap();
    benchmark
        .report_metric(
            &mut reporter,
            "input-bytes",
            JSON_DATA.len() as u64,
            Some("bytes"),
        )
        .unwrap();
    krabi_caliper::avr::park_simavr(&dp.CPU)
}
