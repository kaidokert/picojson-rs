#![cfg_attr(not(test), no_std)]

pub mod bitstack;
pub use bitstack::BitStack;
mod tokenizer;

pub use tokenizer::Tokenizer;
pub use tokenizer::{Event, EventToken};

/// Trait that combines all the required trait bounds for depth counter types.
/// This is automatically implemented for any type that satisfies the individual bounds.
pub trait BitStackCore:
    From<u8>
    + core::cmp::PartialEq<Self>
    + core::ops::AddAssign<Self>
    + core::ops::SubAssign<Self>
    + core::ops::Not<Output = Self>
    + core::fmt::Debug
{
}

impl<T> BitStackCore for T where
    T: From<u8>
        + core::cmp::PartialEq<T>
        + core::ops::AddAssign<T>
        + core::ops::SubAssign<T>
        + core::ops::Not<Output = T>
        + core::fmt::Debug
{
}
