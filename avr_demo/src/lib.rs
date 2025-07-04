#![no_std]

pub mod stack_measurement;

#[inline(never)]
fn inner_panic_handler() -> ! {
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    inner_panic_handler();
}
