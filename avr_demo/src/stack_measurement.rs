use embedded_measure::stack::{Avr, LinkerStack, StackConfig, StackProbe};

unsafe extern "C" {
    static mut _end: u8;
}
// This harness is built and simulated for ATmega2560 by .cargo/config.toml.
const RAMEND_EXCLUSIVE: usize = 0x2200;

pub fn fill_stack_with_watermark() -> StackProbe {
    let stack = unsafe { LinkerStack::new(&raw mut _end, RAMEND_EXCLUSIVE as *mut u8, Avr) };
    StackProbe::paint(&stack, StackConfig::new(64).sentinel(0xce)).unwrap()
}

pub fn measure_stack_usage(probe: &StackProbe) -> u16 {
    probe.measure().high_water_bytes as u16
}
