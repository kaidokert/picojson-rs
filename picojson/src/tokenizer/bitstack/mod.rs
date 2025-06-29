// SPDX-License-Identifier: Apache-2.0

use core::cmp::PartialEq;
use core::ops::{BitAnd, BitOr, Shl, Shr};

/// Trait for bit stacks.
/// This trait is implemented for both integer and [T; N] types.
///
/// NOTE: BitStack implementations do NOT implement depth tracking.
/// This is the responsibility of the caller.
pub trait BitStack {
    /// Returns a default-initialized bit stack.
    fn default() -> Self;
    /// Pushes a bit (true for 1, false for 0) onto the stack.
    fn push(&mut self, bit: bool);
    /// Pops the top bit off the stack, returning it if the stack isn’t empty.
    fn pop(&mut self) -> bool;
    /// Returns the top bit without removing it.
    fn top(&self) -> bool;
}

/// Automatic implementation for builtin-types ( u8, u32 etc ).
/// Any type that implements the required traits is automatically implemented for BitStack.
impl<T> BitStack for T
where
    T: Shl<u8, Output = T>
        + Shr<u8, Output = T>
        + BitAnd<T, Output = T>
        + BitOr<Output = T>
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

// ============================================================================
// NEW API: BitStack Configuration System
// ============================================================================

/// Trait for bit bucket storage - replaces the old BitStack trait name for storage
/// This trait is implemented for both integer and [T; N] types.
/// For now, we make BitBucket extend BitStack for compatibility during migration
pub trait BitBucket: BitStack {
    // BitBucket inherits all methods from BitStack for now
    // This allows gradual migration
}

/// Automatic implementation: any type that implements BitStack also implements BitBucket
impl<T: BitStack> BitBucket for T {}

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
    /// Increment the depth counter
    fn increment(&mut self);
}

impl<T> DepthCounter for T
where
    T: From<u8>
        + core::cmp::PartialEq<T>
        + core::ops::AddAssign<T>
        + core::ops::SubAssign<T>
        + core::ops::Not<Output = T>
        + core::fmt::Debug,
{
    fn increment(&mut self) {
        *self += T::from(1);
    }
}

/// Type alias for ArrayBitBucket - just reuse ArrayBitStack during migration
pub type ArrayBitBucket<const N: usize, T> = ArrayBitStack<N, T>;

/// Configuration trait for BitStack systems - defines bucket and counter types
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

/// Wrapper for arrays to implement BitStack trait.
/// Provides large BitStack storage using multiple elements.
/// This can be used to parse very deeply nested JSON.
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

    fn pop(&mut self) -> bool {
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

        bit
    }

    fn top(&self) -> bool {
        // Return rightmost bit from least significant element without modifying
        (self.0[N - 1].clone() & T::from(1)) != T::from(0)
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
        let mut bitstack: ArrayBitStack<2, u8> = ArrayBitStack::default();

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
        let mut bitstack: ArrayBitStack<10, u32> = ArrayBitStack::default();

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
        let mut bitstack_u8: ArrayBitStack<1, u8> = ArrayBitStack::default();

        // Fill all 8 bits of a u8 element
        for i in 0..8 {
            bitstack_u8.push(i % 2 == 0); // alternating pattern: true, false, true, false...
        }

        // Verify we can retrieve all 8 bits in LIFO order
        for i in (0..8).rev() {
            assert_eq!(bitstack_u8.pop(), i % 2 == 0);
        }

        // Test u32 elements (32-bit each)
        let mut bitstack_u32: ArrayBitStack<1, u32> = ArrayBitStack::default();

        // Fill all 32 bits of a u32 element
        for i in 0..32 {
            bitstack_u32.push(i % 3 == 0); // pattern: true, false, false, true, false, false...
        }

        // Verify we can retrieve all 32 bits in LIFO order
        for i in (0..32).rev() {
            assert_eq!(bitstack_u32.pop(), i % 3 == 0);
        }
    }
}
