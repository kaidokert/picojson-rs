#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use avr_demo::stack_measurement::*;
use krabi_caliper::report::{Field, UfmtReporter};
use krabi_caliper::{Benchmark, CounterPlatform};
use serde::Deserialize;

// Conditional import of uwriteln! - stub out if ufmt feature is not enabled
#[cfg(feature = "ufmt")]
use ufmt::uwriteln;

#[cfg(not(feature = "ufmt"))]
macro_rules! uwriteln {
    ($($args:tt)*) => {
        Ok::<(), core::convert::Infallible>(())
    };
}

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
    let dp = arduino_hal::Peripherals::take().unwrap();
    #[cfg(feature = "ufmt")]
    let serial = {
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    let stack = stack();
    let counter = avr_demo::cyclecount::CycleCounter::start(&dp.TC1);
    let mut platform = CounterPlatform::new(counter);
    let mut reporter = UfmtReporter::new(serial);
    let result = Benchmark::<3>::new("serde-json-core")
        .warmups(1)
        .fields(&[Field::token("target", "atmega2560")])
        .run_with_stack(&mut platform, &mut reporter, &stack, stack_config(), || {
            let mut scratch = [0u8; 1];
            let parsed: Result<(Doc, _), _> =
                serde_json_core::from_slice_escaped(JSON_DATA, &mut scratch);
            parsed.is_ok()
        })
        .unwrap();
    Benchmark::<3>::new("serde-json-core")
        .fields(&[Field::token("target", "atmega2560")])
        .report_metric(
            &mut reporter,
            "input-bytes",
            JSON_DATA.len() as u64,
            Some("bytes"),
        )
        .unwrap();

    let mut serial = reporter.into_inner();
    let stack = result.stack.unwrap();
    uwriteln!(&mut serial, "JSON parsing passed: {}", result.passed).ok();
    uwriteln!(
        &mut serial,
        "Max stack usage: {} bytes",
        stack.high_water_bytes
    )
    .ok();
    uwriteln!(&mut serial, "=== TEST COMPLETE ===").ok();

    avr_device::interrupt::disable();
    loop {
        unsafe { core::arch::asm!("sleep") }
    }
}
