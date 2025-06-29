// SPDX-License-Identifier: Apache-2.0

use ujson::bitstack::{ArrayBitStack, BitStack};

#[test]
fn test_array_bitstack_basic() {
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
fn test_array_bitstack_element_overflow() {
    // Test ArrayBitStack with 2 u8 elements to verify cross-element operations
    let mut bitstack: ArrayBitStack<2, u8> = ArrayBitStack::default();

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
fn test_array_bitstack_empty_behavior() {
    // Test behavior when popping from an empty ArrayBitStack
    // With the new API, empty stacks return false (no depth tracking needed)
    let mut bitstack: ArrayBitStack<2, u8> = ArrayBitStack::default();

    // CURRENT BEHAVIOR: Empty stack returns false (was Some(false) before API change)
    // This behavior is now the intended design - no depth tracking needed
    assert_eq!(bitstack.pop(), false, "Empty stack returns false");
    assert_eq!(bitstack.top(), false, "Empty stack top() returns false");

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
fn test_array_bitstack_underflow_does_not_panic() {
    // Test that multiple underflow attempts are safe (don't panic)
    // This is important for robustness even with the current incorrect API
    let mut bitstack: ArrayBitStack<1, u8> = ArrayBitStack::default();

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
