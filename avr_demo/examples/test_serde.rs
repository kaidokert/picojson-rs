#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo::stack_measurement::*;
use panic_halt as _;
use serde::Deserialize;
use ufmt::uwriteln;

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
    let pins = arduino_hal::pins!(dp);
    let mut serial = arduino_hal::default_serial!(dp, pins, 57600);

    unsafe { fill_stack_with_watermark() };

    let mut scratch = [0u8; 1]; // Use a 1-byte scratch buffer.
    let result: Result<(Doc, _), _> = serde_json_core::from_slice_escaped(JSON_DATA, &mut scratch);

    let stack_used = unsafe { measure_stack_usage() };

    match result {
        Ok((doc, _)) => {
            uwriteln!(&mut serial, "Parsed doc id: {}", doc.id).ok();
            uwriteln!(&mut serial, "Parsed test_depth: {}", doc.test_depth).ok();
        }
        Err(_) => {
            uwriteln!(&mut serial, "JSON parsing failed!").ok();
        }
    }
    uwriteln!(&mut serial, "Max stack usage: {} bytes", stack_used).ok();
    uwriteln!(&mut serial, "=== TEST COMPLETE ===").ok();

    // Exit the simulator
    unsafe { core::arch::asm!("sleep") };
    loop {}
}
