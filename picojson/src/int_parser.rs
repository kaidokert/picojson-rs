// SPDX-License-Identifier: Apache-2.0

// Int parser module, mostly borrowed from core::num::parse::radix

/// A custom error type for const integer parsing.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConstParseIntegerError {
    /// The input byte slice was empty.
    Empty,
    /// The input consisted only of a sign character (`+` or `-`).
    SignOnly,
    /// An invalid character was found that was not a base-10 digit.
    InvalidDigit,
    /// The number overflowed or underflowed the target integer type.
    Overflow,
}

/// Creates a panic-free, const-stable, base-10 parser for a specific signed integer type.
macro_rules! define_const_parser {
    ($fn_name:ident, $int_ty:ty) => {
        /// Parses a byte slice into a(n) `
        #[doc = stringify!($int_ty)]
        /// ` in a `const` context.
        ///
        /// This function is guaranteed not to panic.
        pub const fn $fn_name(src: &[u8]) -> Result<$int_ty, ConstParseIntegerError> {
            // Use pattern matching to safely handle the sign and avoid panics.
            let (is_negative, mut digits) = match src {
                [] => return Err(ConstParseIntegerError::Empty),
                [b'+', rest @ ..] => (false, rest),
                [b'-', rest @ ..] => (true, rest),
                _ => (false, src),
            };

            if digits.is_empty() {
                return Err(ConstParseIntegerError::SignOnly);
            }

            let mut result: $int_ty = 0;

            // Use `while let` with `split_first` for safe, panic-free iteration.
            while let Some((&byte, rest)) = digits.split_first() {
                // Convert byte to digit.
                let digit = match byte {
                    b'0'..=b'9' => (byte - b'0') as $int_ty,
                    // Not a digit, so it's an invalid character.
                    _ => return Err(ConstParseIntegerError::InvalidDigit),
                };

                // Perform checked multiplication.
                result = match result.checked_mul(10) {
                    Some(val) => val,
                    None => return Err(ConstParseIntegerError::Overflow),
                };

                // Perform checked addition or subtraction.
                // Building the number negatively from the start correctly handles iT::MIN.
                if is_negative {
                    result = match result.checked_sub(digit) {
                        Some(val) => val,
                        None => return Err(ConstParseIntegerError::Overflow),
                    }
                } else {
                    result = match result.checked_add(digit) {
                        Some(val) => val,
                        None => return Err(ConstParseIntegerError::Overflow),
                    }
                }

                digits = rest;
            }

            Ok(result)
        }
    };
}

// Generate the functions using the macro.
#[cfg(feature = "int8")]
define_const_parser!(from_ascii_i8, i8);
#[cfg(feature = "int32")]
define_const_parser!(from_ascii_i32, i32);
#[cfg(feature = "int64")]
define_const_parser!(from_ascii_i64, i64);

#[cfg(test)]
mod tests {
    use super::*;

    // --- Tests for from_ascii_i8 ---
    #[cfg(feature = "int8")]
    mod test_i8 {
        use super::*;
        #[test]
        fn test_from_ascii_i8_simple() {
            assert_eq!(from_ascii_i8(b"0"), Ok(0));
            assert_eq!(from_ascii_i8(b"42"), Ok(42));
            assert_eq!(from_ascii_i8(b"-42"), Ok(-42));
            assert_eq!(from_ascii_i8(b"+42"), Ok(42));
        }

        #[test]
        fn test_from_ascii_i8_limits() {
            assert_eq!(from_ascii_i8(b"127"), Ok(i8::MAX));
            assert_eq!(from_ascii_i8(b"-128"), Ok(i8::MIN));
        }

        #[test]
        fn test_from_ascii_i8_overflow() {
            assert_eq!(from_ascii_i8(b"128"), Err(ConstParseIntegerError::Overflow));
            assert_eq!(
                from_ascii_i8(b"-129"),
                Err(ConstParseIntegerError::Overflow)
            );
        }

        #[test]
        fn test_from_ascii_i8_errors() {
            assert_eq!(from_ascii_i8(b""), Err(ConstParseIntegerError::Empty));
            assert_eq!(from_ascii_i8(b"-"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(from_ascii_i8(b"+"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(
                from_ascii_i8(b"12a"),
                Err(ConstParseIntegerError::InvalidDigit)
            );
            assert_eq!(
                from_ascii_i8(b"a12"),
                Err(ConstParseIntegerError::InvalidDigit)
            );
            assert_eq!(
                from_ascii_i8(b"1-2"),
                Err(ConstParseIntegerError::InvalidDigit)
            );
        }
    }

    // --- Tests for from_ascii_i32 ---
    #[cfg(feature = "int32")]
    mod test_i32 {
        use super::*;
        #[test]
        fn test_from_ascii_i32_simple() {
            assert_eq!(from_ascii_i32(b"0"), Ok(0));
            assert_eq!(from_ascii_i32(b"12345"), Ok(12345));
            assert_eq!(from_ascii_i32(b"-12345"), Ok(-12345));
            assert_eq!(from_ascii_i32(b"+12345"), Ok(12345));
        }

        #[test]
        fn test_from_ascii_i32_limits() {
            assert_eq!(
                from_ascii_i32(i32::MAX.to_string().as_bytes()),
                Ok(i32::MAX)
            );
            assert_eq!(
                from_ascii_i32(i32::MIN.to_string().as_bytes()),
                Ok(i32::MIN)
            );
        }

        #[test]
        fn test_from_ascii_i32_overflow() {
            assert_eq!(
                from_ascii_i32(b"2147483648"),
                Err(ConstParseIntegerError::Overflow)
            );
            assert_eq!(
                from_ascii_i32(b"-2147483649"),
                Err(ConstParseIntegerError::Overflow)
            );
        }

        #[test]
        fn test_from_ascii_i32_errors() {
            assert_eq!(from_ascii_i32(b""), Err(ConstParseIntegerError::Empty));
            assert_eq!(from_ascii_i32(b"-"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(from_ascii_i32(b"+"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(
                from_ascii_i32(b"123a45"),
                Err(ConstParseIntegerError::InvalidDigit)
            );
        }
    }

    // --- Tests for from_ascii_i64 ---
    #[cfg(feature = "int64")]
    mod test_i64 {
        use super::*;
        #[test]
        fn test_from_ascii_i64_simple() {
            assert_eq!(from_ascii_i64(b"0"), Ok(0));
            assert_eq!(from_ascii_i64(b"1234567890"), Ok(1234567890));
            assert_eq!(from_ascii_i64(b"-1234567890"), Ok(-1234567890));
            assert_eq!(from_ascii_i64(b"+1234567890"), Ok(1234567890));
        }

        #[test]
        fn test_from_ascii_i64_limits() {
            assert_eq!(
                from_ascii_i64(i64::MAX.to_string().as_bytes()),
                Ok(i64::MAX)
            );
            assert_eq!(
                from_ascii_i64(i64::MIN.to_string().as_bytes()),
                Ok(i64::MIN)
            );
        }

        #[test]
        fn test_from_ascii_i64_overflow() {
            assert_eq!(
                from_ascii_i64(b"9223372036854775808"),
                Err(ConstParseIntegerError::Overflow)
            );
            assert_eq!(
                from_ascii_i64(b"-9223372036854775809"),
                Err(ConstParseIntegerError::Overflow)
            );
        }

        #[test]
        fn test_from_ascii_i64_errors() {
            assert_eq!(from_ascii_i64(b""), Err(ConstParseIntegerError::Empty));
            assert_eq!(from_ascii_i64(b"-"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(from_ascii_i64(b"+"), Err(ConstParseIntegerError::SignOnly));
            assert_eq!(
                from_ascii_i64(b"123a4567890"),
                Err(ConstParseIntegerError::InvalidDigit)
            );
        }
    }
}
