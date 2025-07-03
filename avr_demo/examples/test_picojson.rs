#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo::stack_measurement::*;
use panic_halt as _;
use picojson::{self, Event, ParseError, PullParser, SliceParser};

#[allow(unused_imports)]
use picojson::ArrayBitStack;

// Conditional import of uwriteln! - stub out if ufmt feature is not enabled
#[cfg(feature = "ufmt")]
use ufmt::uwriteln;

#[cfg(not(feature = "ufmt"))]
macro_rules! uwriteln {
    ($($args:tt)*) => {
        Ok::<(), core::convert::Infallible>(())
    };
}

// Conditionally define the configuration based on features.
#[cfg(feature = "pico-small")]
type PicoConfig = ArrayBitStack<64, u8, u16>; // 512 levels
#[cfg(feature = "pico-huge")]
type PicoConfig = ArrayBitStack<256, u8, u16>; // 2048 levels
#[cfg(feature = "pico-tiny")]
type PicoConfig = picojson::DefaultConfig; // 32 levels
                                           // Default config for builds without a feature.
#[cfg(not(any(feature = "pico-small", feature = "pico-huge", feature = "pico-tiny")))]
type PicoConfig = picojson::DefaultConfig;

const JSON_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/test.json"));

struct Doc<'b> {
    id: u32,
    test_depth: u32,
    status: &'b str,
}

fn parse_json<'b>(json_data: &[u8], scratch: &'b mut [u8]) -> Result<Doc<'b>, ParseError> {
    let mut id = 0;
    let mut test_depth = 0;
    let mut status_len = 0;

    let mut pico_scratch = [0u8; 1]; // Use a 1-byte scratch buffer.
    let mut parser =
        SliceParser::<PicoConfig>::with_config_and_buffer_from_slice(json_data, &mut pico_scratch);

    let mut key_is_id = false;
    let mut key_is_test_depth = false;
    let mut key_is_status = false;

    loop {
        match parser.next() {
            Some(Ok(Event::Key(key))) => {
                let s = key.as_str();
                key_is_id = s == "id";
                key_is_test_depth = s == "test_depth";
                key_is_status = s == "status";
            }
            Some(Ok(Event::String(value))) => {
                if key_is_status {
                    let s = value.as_str();
                    status_len = s.len();
                    if let Some(target_slice) = scratch.get_mut(..status_len) {
                        target_slice.copy_from_slice(s.as_bytes());
                    }
                }
                key_is_id = false;
                key_is_test_depth = false;
                key_is_status = false;
            }
            Some(Ok(Event::Number(value))) => {
                if key_is_id {
                    id = value.as_int().unwrap_or(0) as u32;
                } else if key_is_test_depth {
                    test_depth = value.as_int().unwrap_or(0) as u32;
                }
                key_is_id = false;
                key_is_test_depth = false;
                key_is_status = false;
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e),
            None => break,
        }
    }
    let status_str = match scratch
        .get(..status_len)
        .and_then(|slice| core::str::from_utf8(slice).ok())
    {
        Some(s) => s,
        None => "",
    };

    Ok(Doc {
        id,
        test_depth,
        status: status_str,
    })
}

#[arduino_hal::entry]
fn main() -> ! {
    #[cfg(feature = "ufmt")]
    let mut serial = {
        let dp = arduino_hal::Peripherals::take().unwrap();
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    unsafe { fill_stack_with_watermark() };

    let mut scratch = [0u8; 16];
    let result = parse_json(JSON_DATA, &mut scratch);

    let stack_used = unsafe { measure_stack_usage() };

    match result {
        Ok(doc) => {
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
