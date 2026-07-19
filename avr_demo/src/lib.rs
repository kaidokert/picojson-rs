#![no_std]
#![feature(abi_avr_interrupt)]

krabi_caliper::atmega2560_timer1_overflow_handler!();

// Panic handler - registered automatically when crate is imported
#[inline(never)]
fn inner_panic_handler() -> ! {
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    inner_panic_handler();
}
