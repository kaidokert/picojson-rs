#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use avr_demo::stack_measurement::*;
use picojson::{self, Event, ParseError, PullParser, Reader, StreamParser};

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

// Simple Reader implementation for testing that wraps a slice
struct SliceReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }
}

impl<'a> Reader for SliceReader<'a> {
    type Error = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.position);
        if remaining == 0 {
            return Ok(0);
        }

        let to_copy = remaining.min(buf.len());
        let end_pos = self.position.saturating_add(to_copy);
        if let (Some(dest), Some(src)) = (
            buf.get_mut(..to_copy),
            self.data.get(self.position..end_pos),
        ) {
            dest.copy_from_slice(src);
            self.position = end_pos;
            Ok(to_copy)
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum KeyContext {
    None,
    Id,
    TestDepth,
    Status,
}

fn parse_json<'b>(json_data: &[u8], scratch: &'b mut [u8]) -> Result<Doc<'b>, ParseError> {
    let mut id = 0;
    let mut test_depth = 0;
    let mut status_len = 0;

    // Create a streaming buffer for StreamParser (balanced size for testing)
    let mut stream_buffer = [0u8; 96]; // Balanced size for testing
    let reader = SliceReader::new(json_data);
    let mut parser = StreamParser::<_, PicoConfig>::new(reader, &mut stream_buffer);

    let mut key_context = KeyContext::None;

    loop {
        match parser.next() {
            Some(Ok(Event::Key(key))) => {
                let s = key.as_str();
                key_context = match s {
                    "id" => KeyContext::Id,
                    "test_depth" => KeyContext::TestDepth,
                    "status" => KeyContext::Status,
                    _ => KeyContext::None,
                };
            }
            Some(Ok(Event::String(value))) => {
                if key_context == KeyContext::Status {
                    let s = value.as_str();
                    let s_bytes = s.as_bytes();
                    // Only copy if scratch buffer is large enough
                    if s_bytes.len() <= scratch.len() {
                        if let Some(target_slice) = scratch.get_mut(..s_bytes.len()) {
                            target_slice.copy_from_slice(s_bytes);
                            status_len = s_bytes.len(); // Only set length if copy succeeded
                        }
                    }
                }
                key_context = KeyContext::None;
            }
            Some(Ok(Event::Number(value))) => {
                match key_context {
                    KeyContext::Id => id = value.as_int().unwrap_or(0) as u32,
                    KeyContext::TestDepth => test_depth = value.as_int().unwrap_or(0) as u32,
                    _ => {}
                }
                key_context = KeyContext::None;
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e),
            None => break,
        }
    }
    let status_str = scratch
        .get(..status_len)
        .and_then(|slice| core::str::from_utf8(slice).ok())
        .unwrap_or("");

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
            uwriteln!(&mut serial, "StreamParser doc id: {}", doc.id).ok();
            uwriteln!(&mut serial, "StreamParser test_depth: {}", doc.test_depth).ok();
            uwriteln!(&mut serial, "StreamParser status: {}", doc.status).ok();
        }
        Err(_) => {
            uwriteln!(&mut serial, "StreamParser JSON parsing failed!").ok();
        }
    }
    uwriteln!(
        &mut serial,
        "StreamParser max stack usage: {} bytes",
        stack_used
    )
    .ok();
    uwriteln!(&mut serial, "=== STREAMPARSER TEST COMPLETE ===").ok();

    // Exit the simulator
    unsafe { core::arch::asm!("sleep") };
    loop {}
}
