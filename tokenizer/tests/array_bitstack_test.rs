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
        assert_eq!(bitstack.pop(), Some(expected));
    }
}
