// SPDX-License-Identifier: Apache-2.0

use core::cmp::PartialEq;
use core::ops::{BitAnd, BitOr, Shl, Shr};

/// Trait for bit buckets - provides bit storage for JSON parser state.
/// This trait is implemented for both integer and [T; N] types.
///
/// NOTE: BitBucket implementations do NOT implement depth tracking.
/// This is the responsibility of the caller.
pub trait BitBucket: Default {
    /// Pushes a bit (true for 1, false for 0) onto the stack.
    fn push(&mut self, bit: bool);
    /// Pops the top bit off the stack, returning it if the stack isn’t empty.
    fn pop(&mut self) -> bool;
    /// Returns the top bit without removing it.
    fn top(&self) -> bool;
}

/// Automatic implementation for builtin-types ( u8, u32 etc ).
/// Any type that implements the required traits is automatically implemented for BitBucket.
impl<T> BitBucket for T
where
    T: Shl<u8, Output = T>
        + Shr<u8, Output = T>
        + BitAnd<T, Output = T>
        + BitOr<Output = T>
        + PartialEq
        + Clone
        + Default,
    T: From<u8>, // To create 0 and 1 constants
{
    fn push(&mut self, bit: bool) {
        *self = (self.clone() << 1u8) | T::from(bit as u8);
    }

    fn pop(&mut self) -> bool {
        let bit = (self.clone() & T::from(1)) != T::from(0);
        *self = self.clone() >> 1u8;
        bit
    }

    fn top(&self) -> bool {
        (self.clone() & T::from(1)) != T::from(0)
    }
}

// TODO: Can this be implemented for slices as well ?

/// Trait for depth counters - tracks nesting depth
/// This is automatically implemented for any type that satisfies the individual bounds.
pub trait DepthCounter:
    From<u8>
    + core::cmp::PartialEq<Self>
    + core::ops::AddAssign<Self>
    + core::ops::SubAssign<Self>
    + core::ops::Not<Output = Self>
    + core::fmt::Debug
{
}

impl<T> DepthCounter for T where
    T: From<u8>
        + core::cmp::PartialEq<T>
        + core::ops::AddAssign<T>
        + core::ops::SubAssign<T>
        + core::ops::Not<Output = T>
        + core::fmt::Debug
{
}

/// Configuration trait for BitStack systems - defines bucket and counter types.
pub trait BitStackConfig {
    type Bucket: BitBucket + Default;
    type Counter: DepthCounter + Default;
}

/// Default configuration using u32 bucket and u8 counter
pub struct DefaultConfig;

impl BitStackConfig for DefaultConfig {
    type Bucket = u32;
    type Counter = u8;
}

/// User-facing BitStack configuration struct - the main API users interact with
/// Usage: `BitStack<u64, u16>` for custom bucket and counter types
pub struct BitStackStruct<B, C> {
    _phantom: core::marker::PhantomData<(B, C)>,
}

impl<B, C> BitStackConfig for BitStackStruct<B, C>
where
    B: BitBucket + Default,
    C: DepthCounter + Default,
{
    type Bucket = B;
    type Counter = C;
}

/// Array-based BitStack implementation for large storage capacity.
pub type ArrayBitStack<const N: usize, T, D> = BitStackStruct<ArrayBitBucket<N, T>, D>;

/// Array-based BitBucket implementation for large storage capacity.
/// Provides large BitBucket storage using multiple elements.
/// This can be used to parse very deeply nested JSON.
#[derive(Debug)]
pub struct ArrayBitBucket<const N: usize, T>(pub [T; N]);

impl<const N: usize, T: Default + Copy> Default for ArrayBitBucket<N, T> {
    fn default() -> Self {
        ArrayBitBucket([T::default(); N])
    }
}

impl<const N: usize, T> BitBucket for ArrayBitBucket<N, T>
where
    T: Shl<u8, Output = T>
        + Shr<u8, Output = T>
        + BitAnd<T, Output = T>
        + core::ops::BitOr<Output = T>
        + PartialEq
        + Clone
        + From<u8>
        + Copy
        + Default,
{
    fn push(&mut self, bit: bool) {
        // Strategy: Use array as big-endian storage, with leftmost element as most significant
        // Shift all elements left, carrying overflow from right to left
        let bit_val = T::from(bit as u8);
        let mut carry = bit_val;
        let element_bits = (core::mem::size_of::<T>() * 8) as u8;
        let msb_shift = element_bits - 1;

        // Start from the rightmost (least significant) element and work left
        for i in (0..N).rev() {
            let old_msb = (self.0[i] >> msb_shift) & T::from(1); // Extract MSB that will be lost
            self.0[i] = (self.0[i] << 1u8) | carry;
            carry = old_msb;
        }
        // Note: carry from leftmost element is discarded (overflow)
    }

    fn pop(&mut self) -> bool {
        // Extract rightmost bit from least significant element
        let bit = (self.0[N - 1] & T::from(1)) != T::from(0);

        // Shift all elements right, carrying underflow from left to right
        let mut carry = T::from(0);
        let element_bits = (core::mem::size_of::<T>() * 8) as u8;
        let msb_shift = element_bits - 1;

        // Start from the leftmost (most significant) element and work right
        for i in 0..N {
            let old_lsb = self.0[i] & T::from(1); // Extract LSB that will be lost
            self.0[i] = (self.0[i] >> 1u8) | (carry << msb_shift);
            carry = old_lsb;
        }

        bit
    }

    fn top(&self) -> bool {
        // Return rightmost bit from least significant element without modifying
        (self.0[N - 1] & T::from(1)) != T::from(0)
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
        assert_eq!(bitstack.pop(), false);
        assert_eq!(bitstack.pop(), true);
    }

    #[test]
    fn test_array_bitstack() {
        // Test ArrayBitStack with 2 u8 elements (16-bit total capacity)
        let mut bitstack: ArrayBitBucket<2, u8> = ArrayBitBucket::default();

        // Test basic push/pop operations
        bitstack.push(true);
        bitstack.push(false);
        bitstack.push(true);

        // Verify top() doesn't modify stack
        assert_eq!(bitstack.top(), true);
        assert_eq!(bitstack.top(), true);

        // Verify LIFO order
        assert_eq!(bitstack.pop(), true);
        assert_eq!(bitstack.pop(), false);
        assert_eq!(bitstack.pop(), true);
    }

    #[test]
    fn test_array_bitstack_large_capacity() {
        // Test larger ArrayBitStack (320-bit capacity with 10 u32 elements)
        let mut bitstack: ArrayBitBucket<10, u32> = ArrayBitBucket::default();

        // Push many bits to test multi-element handling
        let pattern = [true, false, true, true, false, false, true, false];
        for &bit in &pattern {
            bitstack.push(bit);
        }

        // Pop and verify reverse order (LIFO)
        for &expected in pattern.iter().rev() {
            assert_eq!(bitstack.pop(), expected);
        }
    }

    #[test]
    fn test_element_size_handling() {
        // Test that bitstack correctly handles different element sizes

        // Test u8 elements (8-bit each)
        let mut bitstack_u8: ArrayBitBucket<1, u8> = ArrayBitBucket::default();

        // Fill all 8 bits of a u8 element
        for i in 0..8 {
            bitstack_u8.push(i % 2 == 0); // alternating pattern: true, false, true, false...
        }

        // Verify we can retrieve all 8 bits in LIFO order
        for i in (0..8).rev() {
            assert_eq!(bitstack_u8.pop(), i % 2 == 0);
        }

        // Test u32 elements (32-bit each)
        let mut bitstack_u32: ArrayBitBucket<1, u32> = ArrayBitBucket::default();

        // Fill all 32 bits of a u32 element
        for i in 0..32 {
            bitstack_u32.push(i % 3 == 0); // pattern: true, false, false, true, false, false...
        }

        // Verify we can retrieve all 32 bits in LIFO order
        for i in (0..32).rev() {
            assert_eq!(bitstack_u32.pop(), i % 3 == 0);
        }
    }

    #[test]
    fn test_array_bitstack_basic_moved() {
        // Test ArrayBitStack with 2 u8 elements (16-bit total capacity)
        let mut bitstack: ArrayBitBucket<2, u8> = ArrayBitBucket::default();

        // Test basic push/pop operations
        bitstack.push(true);
        bitstack.push(false);
        bitstack.push(true);

        // Verify top() doesn't modify stack
        assert_eq!(bitstack.top(), true);
        assert_eq!(bitstack.top(), true);

        // Verify LIFO order
        assert_eq!(bitstack.pop(), true);
        assert_eq!(bitstack.pop(), false);
        assert_eq!(bitstack.pop(), true);
    }

    #[test]
    fn test_array_bitstack_large_capacity_moved() {
        // Test larger ArrayBitStack (320-bit capacity with 10 u32 elements)
        let mut bitstack: ArrayBitBucket<10, u32> = ArrayBitBucket::default();

        // Push many bits to test multi-element handling
        let pattern = [true, false, true, true, false, false, true, false];
        for &bit in &pattern {
            bitstack.push(bit);
        }

        // Pop and verify reverse order (LIFO)
        for &expected in pattern.iter().rev() {
            assert_eq!(bitstack.pop(), expected);
        }
    }

    #[test]
    fn test_array_bitstack_element_overflow_moved() {
        // Test ArrayBitStack with 2 u8 elements to verify cross-element operations
        let mut bitstack: ArrayBitBucket<2, u8> = ArrayBitBucket::default();

        // Push more than 8 bits to force usage of multiple elements
        let bits = [
            true, false, true, false, true, false, true, false, true, true,
        ];
        for &bit in &bits {
            bitstack.push(bit);
        }

        // Pop all bits and verify order
        for &expected in bits.iter().rev() {
            assert_eq!(bitstack.pop(), expected);
        }
    }

    #[test]
    fn test_array_bitstack_empty_behavior_moved() {
        // Test behavior when popping from an empty ArrayBitStack
        // With the new API, empty stacks return false (no depth tracking needed)
        let mut bitstack: ArrayBitBucket<2, u8> = ArrayBitBucket::default();

        // CURRENT BEHAVIOR: Empty stack returns false (was Some(false) before API change)
        // This behavior is now the intended design - no depth tracking needed
        assert!(!bitstack.pop(), "Empty stack returns false");
        assert!(!bitstack.top(), "Empty stack top() returns false");

        // Test that underflow doesn't panic (at least it's safe)
        assert_eq!(
            bitstack.pop(),
            false,
            "Multiple underflow calls don't panic"
        );
        assert_eq!(
            bitstack.pop(),
            false,
            "Multiple underflow calls don't panic"
        );
    }

    #[test]
    fn test_array_bitstack_underflow_does_not_panic_moved() {
        // Test that multiple underflow attempts are safe (don't panic)
        // This is important for robustness even with the current incorrect API
        let mut bitstack: ArrayBitBucket<1, u8> = ArrayBitBucket::default();

        // Multiple calls to pop() on empty stack should not panic
        for i in 0..5 {
            let result = bitstack.pop();
            // With new API, just ensure it doesn't panic and returns a bool
            assert_eq!(
                result,
                false,
                "Empty ArrayBitStack pop() attempt {} should return false",
                i + 1
            );

            let top_result = bitstack.top();
            assert_eq!(
                top_result,
                false,
                "Empty ArrayBitStack top() attempt {} should return false",
                i + 1
            );
        }
    }
}
