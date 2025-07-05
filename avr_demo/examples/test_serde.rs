#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use avr_demo::stack_measurement::*;
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
    #[cfg(feature = "ufmt")]
    let mut serial = {
        let dp = arduino_hal::Peripherals::take().unwrap();
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    unsafe { fill_stack_with_watermark() };

    let mut scratch = [0u8; 1]; // Use a 1-byte scratch buffer.
    let result: Result<(Doc, _), _> = serde_json_core::from_slice_escaped(JSON_DATA, &mut scratch);

    let stack_used = unsafe { measure_stack_usage() };

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
    uwriteln!(&mut serial, "Max stack usage: {} bytes", stack_used).ok();
    uwriteln!(&mut serial, "=== TEST COMPLETE ===").ok();

    // Exit the simulator
    unsafe { core::arch::asm!("sleep") };
    loop {}
}
