#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use avr_demo::stack_measurement::*;
use embedded_measure::avr::timer_measurement;
use embedded_measure::report::{
    Field, MeasurementRecord, OutcomeRecord, StackRecord, write_measurement_ufmt,
    write_outcome_ufmt, write_stack_ufmt,
};
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
    let mut serial = {
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    let stack_probe = fill_stack_with_watermark();

    let counter = avr_demo::cyclecount::CycleCounter::start(&dp.TC1);
    let mut scratch = [0u8; 1]; // Use a 1-byte scratch buffer.
    let result: Result<(Doc, _), _> = serde_json_core::from_slice_escaped(JSON_DATA, &mut scratch);
    let passed = result.is_ok();
    let ticks = counter.elapsed_ticks(&dp.TC1);

    let stack = measure_stack(&stack_probe);
    write_stack_ufmt(
        &mut serial,
        &StackRecord {
            benchmark: "serde-json-core",
            measurement: stack,
            fields: &[Field::token("target", "atmega2560")],
        },
    )
    .unwrap();
    write_outcome_ufmt(
        &mut serial,
        &OutcomeRecord {
            benchmark: "serde-json-core",
            passed,
            fields: &[Field::token("target", "atmega2560")],
        },
    )
    .unwrap();
    write_measurement_ufmt(
        &mut serial,
        &MeasurementRecord {
            benchmark: "serde-json-core",
            measurement: timer_measurement(ticks, 15_625, false),
            fields: &[Field::token("target", "atmega2560")],
        },
    )
    .unwrap();

    match result {
        Ok((doc, _)) => {
            uwriteln!(&mut serial, "Parsed doc id: {}", doc.id).ok();
            uwriteln!(&mut serial, "Parsed test_depth: {}", doc.test_depth).ok();
            uwriteln!(&mut serial, "Parsed status: {}", doc.status).ok();
        }
        Err(_) => {
            uwriteln!(&mut serial, "JSON parsing failed!").ok();
        }
    }
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
