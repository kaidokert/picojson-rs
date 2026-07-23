#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;
use krabi_caliper::report::Field;
#[cfg(feature = "ufmt")]
use krabi_caliper::report::UfmtReporter;
use krabi_caliper::stack::{Avr, LinkerStack, StackConfig};
use krabi_caliper::Benchmark;
use picojson::{self, ChunkReader, Event, ParseError, PullParser, StreamParser};

#[allow(unused_imports)]
use picojson::ArrayBitStack;

// Conditionally define the configuration based on features.
#[cfg(feature = "pico-tiny")]
type PicoConfig = picojson::DefaultConfig; // 32 levels
#[cfg(feature = "pico-small")]
type PicoConfig = ArrayBitStack<64, u8, u16>; // 512 levels
#[cfg(feature = "pico-huge")]
type PicoConfig = ArrayBitStack<256, u8, u16>; // 2048 levels
                                               // Default config for builds without a feature.
#[cfg(not(any(feature = "pico-small", feature = "pico-huge", feature = "pico-tiny")))]
type PicoConfig = picojson::DefaultConfig;

const JSON_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/test.json"));

/// Helper function for byte-by-byte copy that won't panic
/// Returns Ok(bytes_copied) on success, or Err(failed_index) if bounds check fails
fn copy_subslice(
    src: &[u8],
    src_start: usize,
    dest: &mut [u8],
    dest_start: usize,
    copy_len: usize,
) -> Result<usize, usize> {
    // Calculate end positions once with overflow checking
    let src_end = src_start.checked_add(copy_len).ok_or(0usize)?;
    let dest_end = dest_start.checked_add(copy_len).ok_or(0usize)?;

    // Get both slices safely (they handle bounds checking)
    match (
        src.get(src_start..src_end),
        dest.get_mut(dest_start..dest_end),
    ) {
        (Some(src_slice), Some(dest_slice)) => {
            // Copy byte by byte, don't use iterators to avoid panics
            for i in 0..src_slice.len() {
                match (dest_slice.get_mut(i), src_slice.get(i)) {
                    (Some(dst), Some(src)) => *dst = *src,
                    _ => return Err(i), // Return the index where either access failed
                }
            }
            Ok(copy_len)
        }
        _ => Err(0), // Either slice extraction failed
    }
}

#[allow(dead_code)]
struct Doc<'b> {
    id: u32,
    test_depth: u32,
    status: &'b str,
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
    let mut stream_buffer = [0u8; 12];
    let reader = ChunkReader::full_slice(json_data);
    let mut parser = StreamParser::<_, PicoConfig>::with_config(reader, &mut stream_buffer);

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
                    let s_bytes = value.as_str().as_bytes();
                    if let Ok(copied) = copy_subslice(s_bytes, 0, scratch, 0, s_bytes.len()) {
                        status_len = copied; // Only set length if copy succeeded
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
    let mut dp = arduino_hal::Peripherals::take().unwrap();
    #[cfg(feature = "ufmt")]
    let serial = {
        let pins = arduino_hal::pins!(dp);
        arduino_hal::default_serial!(dp, pins, 57600)
    };

    #[cfg(feature = "ufmt")]
    let mut reporter = UfmtReporter::new(serial);
    #[cfg(not(feature = "ufmt"))]
    let mut reporter = avr_demo::NullReporter;
    let fields = [Field::token("architecture", "atmega2560")];
    let benchmark = Benchmark::<3>::new("picojson-stream-parser")
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
                let mut scratch = [0u8; 16];
                parse_json(JSON_DATA, &mut scratch).is_ok()
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
