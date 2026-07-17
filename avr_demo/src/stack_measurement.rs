use embedded_measure::stack::{Avr, LinkerStack, StackConfig};

unsafe extern "C" {
    static mut _end: u8;
}
// This harness is built and simulated for ATmega2560 by .cargo/config.toml.
const RAMEND_EXCLUSIVE: usize = 0x2200;

pub fn stack() -> LinkerStack<Avr> {
    unsafe { LinkerStack::new(&raw mut _end, RAMEND_EXCLUSIVE as *mut u8, Avr) }
}

pub const fn stack_config() -> StackConfig {
    StackConfig::new(64).sentinel(0xce)
}
