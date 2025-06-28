use core::cmp::PartialEq;
use core::ops::{BitAnd, Shl, Shr};

pub trait BitStack {
    fn default() -> Self;
    /// Pushes a bit (true for 1, false for 0) onto the stack.
    fn push(&mut self, bit: bool);
    /// Pops the top bit off the stack, returning it if the stack isn’t empty.
    fn pop(&mut self) -> Option<bool>;
    /// Returns the top bit without removing it, or None if empty.
    fn top(&self) -> Option<bool>;
}

impl<T> BitStack for T
where
    T: Shl<u8, Output = T>
        + Shr<u8, Output = T>
        + BitAnd<T, Output = T>
        + core::ops::BitOr<Output = T>
        + PartialEq
        + Clone,
    T: From<u8>, // To create 0 and 1 constants
{
    fn default() -> Self {
        T::from(0)
    }
    fn push(&mut self, bit: bool) {
        *self = (self.clone() << 1u8) | T::from(bit as u8);
    }

    fn pop(&mut self) -> Option<bool> {
        let bit = (self.clone() & T::from(1)) != T::from(0);
        *self = self.clone() >> 1u8;
        Some(bit)
    }

    fn top(&self) -> Option<bool> {
        Some((self.clone() & T::from(1)) != T::from(0))
    }
}

// Newtype wrapper for arrays to implement BitStack trait
// Provides large BitStack storage using multiple elements
#[derive(Debug)]
pub struct ArrayBitStack<const N: usize, T>(pub [T; N]);

impl<const N: usize, T> BitStack for ArrayBitStack<N, T>
where
    T: Shl<u8, Output = T>
        + Shr<u8, Output = T>
        + BitAnd<T, Output = T>
        + core::ops::BitOr<Output = T>
        + PartialEq
        + Clone
        + From<u8>,
{
    fn default() -> Self {
        ArrayBitStack(core::array::from_fn(|_| T::from(0)))
    }

    fn push(&mut self, bit: bool) {
        // Strategy: Use array as big-endian storage, with leftmost element as most significant
        // Shift all elements left, carrying overflow from right to left
        let bit_val = T::from(bit as u8);
        let mut carry = bit_val;
        let element_bits = (core::mem::size_of::<T>() * 8) as u8;
        let msb_shift = element_bits - 1;

        // Start from the rightmost (least significant) element and work left
        for i in (0..N).rev() {
            let old_msb = (self.0[i].clone() >> msb_shift) & T::from(1); // Extract MSB that will be lost
            self.0[i] = (self.0[i].clone() << 1u8) | carry;
            carry = old_msb;
        }
        // Note: carry from leftmost element is discarded (overflow)
    }

    fn pop(&mut self) -> Option<bool> {
        // Extract rightmost bit from least significant element
        let bit = (self.0[N - 1].clone() & T::from(1)) != T::from(0);

        // Shift all elements right, carrying underflow from left to right
        let mut carry = T::from(0);
        let element_bits = (core::mem::size_of::<T>() * 8) as u8;
        let msb_shift = element_bits - 1;

        // Start from the leftmost (most significant) element and work right
        for i in 0..N {
            let old_lsb = self.0[i].clone() & T::from(1); // Extract LSB that will be lost
            self.0[i] = (self.0[i].clone() >> 1u8) | (carry << msb_shift);
            carry = old_lsb;
        }

        Some(bit)
    }

    fn top(&self) -> Option<bool> {
        // Return rightmost bit from least significant element without modifying
        Some((self.0[N - 1].clone() & T::from(1)) != T::from(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitstack() {
        let mut bitstack = 0;
        bitstack.push(true);
        bitstack.push(false);
        assert_eq!(bitstack.pop(), Some(false));
        assert_eq!(bitstack.pop(), Some(true));
    }

    #[test]
    fn test_array_bitstack() {
        // Test ArrayBitStack with 2 u8 elements (16-bit total capacity)
        let mut bitstack: ArrayBitStack<2, u8> = ArrayBitStack::default();

        // Test basic push/pop operations
        bitstack.push(true);
        bitstack.push(false);
        bitstack.push(true);

        // Verify top() doesn't modify stack
        assert_eq!(bitstack.top(), Some(true));
        assert_eq!(bitstack.top(), Some(true));

        // Verify LIFO order
        assert_eq!(bitstack.pop(), Some(true));
        assert_eq!(bitstack.pop(), Some(false));
        assert_eq!(bitstack.pop(), Some(true));
    }

    #[test]
    fn test_array_bitstack_large_capacity() {
        // Test larger ArrayBitStack (320-bit capacity with 10 u32 elements)
        let mut bitstack: ArrayBitStack<10, u32> = ArrayBitStack::default();

        // Push many bits to test multi-element handling
        let pattern = [true, false, true, true, false, false, true, false];
        for &bit in &pattern {
            bitstack.push(bit);
        }

        // Pop and verify reverse order (LIFO)
        for &expected in pattern.iter().rev() {
            assert_eq!(bitstack.pop(), Some(expected));
        }
    }

    #[test]
    fn test_element_size_handling() {
        // Test that bitstack correctly handles different element sizes

        // Test u8 elements (8-bit each)
        let mut bitstack_u8: ArrayBitStack<1, u8> = ArrayBitStack::default();

        // Fill all 8 bits of a u8 element
        for i in 0..8 {
            bitstack_u8.push(i % 2 == 0); // alternating pattern: true, false, true, false...
        }

        // Verify we can retrieve all 8 bits in LIFO order
        for i in (0..8).rev() {
            assert_eq!(bitstack_u8.pop(), Some(i % 2 == 0));
        }

        // Test u32 elements (32-bit each)
        let mut bitstack_u32: ArrayBitStack<1, u32> = ArrayBitStack::default();

        // Fill all 32 bits of a u32 element
        for i in 0..32 {
            bitstack_u32.push(i % 3 == 0); // pattern: true, false, false, true, false, false...
        }

        // Verify we can retrieve all 32 bits in LIFO order
        for i in (0..32).rev() {
            assert_eq!(bitstack_u32.pop(), Some(i % 3 == 0));
        }
    }
}
