// The magic value we'll use to fill the stack area.
const STACK_WATERMARK: u8 = 0xCE;

// For the ATmega2560, RAMEND is at address 0x21FF.
const RAMEND_ADDR: u16 = 0x21FF;

// Linker symbol that marks the end of the .bss section.
unsafe extern "C" {
    static mut _end: u8;
}

/// Fills the unused RAM with a magic value.
pub unsafe fn fill_stack_with_watermark() {
    let stack_start_ptr = &raw mut _end as *mut u8;
    let stack_end_ptr = RAMEND_ADDR as *mut u8;

    // Even inside an `unsafe fn`, these operations now require an `unsafe` block.
    unsafe {
        let mut current_ptr = stack_start_ptr;
        while current_ptr <= stack_end_ptr {
            core::ptr::write_volatile(current_ptr, STACK_WATERMARK);
            current_ptr = current_ptr.add(1);
        }
    }
}

/// Measures the maximum stack usage by finding the "high-water mark".
/// This is unsafe because we are reading from a large, arbitrary memory region.
pub unsafe fn measure_stack_usage() -> u16 {
    let stack_start_ptr = &raw const _end as *const u8;
    let stack_end_ptr = RAMEND_ADDR as *const u8;

    // Validate memory region bounds before proceeding
    if stack_start_ptr > stack_end_ptr {
        return 0; // Invalid memory layout
    }

    unsafe {
        let mut current_ptr = stack_start_ptr;

        // Add explicit bounds checking in the loop condition
        while current_ptr < stack_end_ptr {
            // Validate pointer is still within bounds before reading
            if current_ptr > stack_end_ptr {
                break; // Safety check - should not happen but prevents overflow
            }

            if core::ptr::read_volatile(current_ptr) != STACK_WATERMARK {
                // We found the first byte that was overwritten. This is our high-water mark.
                // The stack grows downwards from RAMEND, so the usage is the distance from the top.
                return (stack_end_ptr as u16) - (current_ptr as u16);
            }

            // Check for potential overflow before incrementing
            if current_ptr == stack_end_ptr {
                break; // At boundary, prevent overflow
            }

            current_ptr = current_ptr.add(1);
        }

        // Handle edge case: check the final byte at stack_end_ptr
        if current_ptr == stack_end_ptr && core::ptr::read_volatile(current_ptr) != STACK_WATERMARK
        {
            return (stack_end_ptr as u16) - (current_ptr as u16);
        }
    }

    0 // Should not happen if stack was used at all.
}
