#![no_std]
#![feature(abi_avr_interrupt)]

pub mod stack_measurement;
pub mod cyclecount;

// Panic handler - registered automatically when crate is imported
#[inline(never)]
fn inner_panic_handler() -> ! {
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    inner_panic_handler();
}
