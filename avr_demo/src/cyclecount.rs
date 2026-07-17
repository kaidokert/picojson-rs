//! Wrap-counted ATmega2560 Timer/Counter1 adapter.

use arduino_hal::pac::TC1;
use avr_device::interrupt::Mutex;
use core::cell::Cell;
use embedded_measure::{Counter, Measurement, Unit};

static TIMER1_WRAPS: Mutex<Cell<u32>> = Mutex::new(Cell::new(0));

#[avr_device::interrupt(atmega2560)]
fn TIMER1_OVF() {
    avr_device::interrupt::free(|cs| {
        let wraps = TIMER1_WRAPS.borrow(cs);
        wraps.set(wraps.get().wrapping_add(1));
    });
}

fn read_total(tc1: &TC1) -> u64 {
    avr_device::interrupt::free(|cs| {
        embedded_measure::avr::extend_timer16(
            TIMER1_WRAPS.borrow(cs).get(),
            tc1.tcnt1.read().bits(),
            tc1.tifr1.read().tov1().bit_is_set(),
        )
    })
}

pub struct CycleCounter<'a> {
    tc1: &'a TC1,
}

impl<'a> CycleCounter<'a> {
    pub fn start(tc1: &'a TC1) -> Self {
        avr_device::interrupt::free(|cs| TIMER1_WRAPS.borrow(cs).set(0));
        tc1.tccr1b.write(|w| w.cs1().prescale_1024());
        tc1.tifr1.write(|w| w.tov1().set_bit());
        tc1.timsk1.write(|w| w.toie1().set_bit());
        unsafe { avr_device::interrupt::enable() };
        Self { tc1 }
    }
}

impl Counter for CycleCounter<'_> {
    type Instant = u64;

    fn now(&mut self) -> Self::Instant {
        read_total(self.tc1)
    }

    fn elapsed(&mut self, start: Self::Instant) -> Measurement {
        Measurement::new(read_total(self.tc1).wrapping_sub(start), Unit::TimerTicks)
            .with_frequency(15_625)
    }
}
