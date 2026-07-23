#![no_std]
#![feature(abi_avr_interrupt)]

krabi_caliper::atmega2560_timer1_overflow_handler!();

/// Reporter used by build-only footprint analysis that deliberately omits UART.
pub struct NullReporter;

impl krabi_caliper::report::Reporter for NullReporter {
    type Error = core::convert::Infallible;

    fn run_start(&mut self, _: &krabi_caliper::report::RunStart<'_>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn sample(&mut self, _: &krabi_caliper::report::SampleRecord<'_>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn result(
        &mut self,
        _: &krabi_caliper::report::ComparisonRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn diagnostic(
        &mut self,
        _: &krabi_caliper::report::ComparisonRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn run_summary(
        &mut self,
        _: &krabi_caliper::report::RunSummary<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn measurement(
        &mut self,
        _: &krabi_caliper::report::MeasurementRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn indexed_measurement(
        &mut self,
        _: &krabi_caliper::report::MeasurementRecord<'_>,
        _: u32,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn outcome(&mut self, _: &krabi_caliper::report::OutcomeRecord<'_>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn boundary(
        &mut self,
        _: &krabi_caliper::report::BoundaryRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn counter_snapshot(
        &mut self,
        _: &krabi_caliper::report::CounterSnapshotRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn metric(&mut self, _: &krabi_caliper::report::MetricRecord<'_>) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl krabi_caliper::report::StackReporter for NullReporter {
    fn stack_measurement(
        &mut self,
        _: &krabi_caliper::report::StackRecord<'_>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
///
/// # Safety
/// `out` must be valid and aligned for one writable `u8`.
pub unsafe extern "C" fn panic_audit__neg__bounds_check(index: usize, out: *mut u8) {
    let values = core::hint::black_box([0u8; 4]);
    let value = values[core::hint::black_box(index)];
    unsafe { *out = core::hint::black_box(value) };
}

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
///
/// # Safety
/// `out` must be valid and aligned for one writable `u8`.
pub unsafe extern "C" fn panic_audit__neg__unwrap(out: *mut u8) {
    let value = core::hint::black_box(None::<u8>).unwrap();
    unsafe { *out = core::hint::black_box(value) };
}

#[cfg(feature = "neg-controls")]
#[inline(never)]
#[unsafe(no_mangle)]
///
/// # Safety
/// `out` must be valid and aligned for one writable `u8`.
pub unsafe extern "C" fn panic_audit__neg__expect(out: *mut u8) {
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
