#![no_std]
#![feature(abi_avr_interrupt)]

krabi_caliper::atmega2560_timer1_overflow_handler!();

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn panic_audit__neg__bounds_check(index: usize, out: *mut u8) {
    let values = core::hint::black_box([0u8; 4]);
    let value = values[core::hint::black_box(index)];
    unsafe { *out = core::hint::black_box(value) };
}

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn panic_audit__neg__unwrap(out: *mut u8) {
    let value = core::hint::black_box(None::<u8>).unwrap();
    unsafe { *out = core::hint::black_box(value) };
}

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn panic_audit__neg__expect(out: *mut u8) {
    let value = core::hint::black_box(None::<u8>).expect("panic audit negative control");
    unsafe { *out = core::hint::black_box(value) };
}

// Panic handler - registered automatically when crate is imported
#[inline(never)]
fn inner_panic_handler() -> ! {
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    inner_panic_handler();
}
