#![feature(asm_experimental_arch)]
#![no_std]
#![no_main]

use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    loop {
        unsafe { core::arch::asm!("sleep") }
    }
}
