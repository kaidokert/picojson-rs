use krabi_caliper::stack::{Avr, LinkerStack, StackConfig};

// This harness is built and simulated for ATmega2560 by .cargo/config.toml.
const RAMEND_EXCLUSIVE: usize = 0x2200;

pub fn stack() -> LinkerStack<Avr> {
    unsafe { LinkerStack::<Avr>::avr_runtime(RAMEND_EXCLUSIVE) }
}

pub const fn stack_config() -> StackConfig {
    StackConfig::new(64).sentinel(0xce)
}
