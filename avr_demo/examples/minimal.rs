#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use avr_demo as _;

#[arduino_hal::entry]
fn main() -> ! {
    #[cfg(feature = "neg-controls")]
    {
        let mut output = 0u8;
        avr_demo::panic_audit__neg__bounds_check(core::hint::black_box(usize::MAX), &mut output);
        avr_demo::panic_audit__neg__unwrap(&mut output);
        avr_demo::panic_audit__neg__expect(&mut output);
        core::hint::black_box(output);
    }
    loop {
        unsafe { core::arch::asm!("sleep") }
    }
}
