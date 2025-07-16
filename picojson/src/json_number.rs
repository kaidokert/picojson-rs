// SPDX-License-Identifier: Apache-2.0

use core::ops::Deref;
use core::str::FromStr;

use crate::ParseError;

#[cfg(feature = "int32")]
use crate::int_parser::from_ascii_i32;
#[cfg(feature = "int64")]
use crate::int_parser::from_ascii_i64;
#[cfg(feature = "int8")]
use crate::int_parser::from_ascii_i8;

// Type alias for the configured integer type
#[cfg(feature = "int8")]
type ConfiguredInt = i8;
#[cfg(feature = "int32")]
type ConfiguredInt = i32;
#[cfg(feature = "int64")]
type ConfiguredInt = i64;

/// Represents the parsed result of a JSON number.
///
/// Depending on crate configuration for float and integer support,
/// variants like `FloatDisabled`, `FloatSkipped` and `FloatTruncated` are
/// conditionally available.
#[derive(Debug, PartialEq)]
pub enum NumberResult {
    /// Integer that fits in the configured integer type
    Integer(ConfiguredInt),
    /// Integer too large for configured type (use raw string for exact representation)
    IntegerOverflow,
    /// Float value (only available with float feature)
    Float(f64),
    /// Float parsing disabled - behavior depends on configuration
    FloatDisabled,
    /// Float encountered but skipped due to float-skip configuration
    FloatSkipped,
    /// Float truncated to integer due to float-truncate configuration
    FloatTruncated(ConfiguredInt),
}

/// Represents a JSON number with both exact string representation and parsed value.
///
/// This preserves the exact number string from the tokenizer while providing
/// convenient access to parsed representations based on compilation features.
///
/// Lifetimes: 'a is the input slice lifetime, 'b is the scratch/copy buffer lifetime
#[derive(Debug, PartialEq)]
pub enum JsonNumber<'a, 'b> {
    /// A raw slice from the original input, used when no copying is needed.
    Borrowed { raw: &'a str, parsed: NumberResult },
    /// A slice from the scratch/copy buffer, used when number had to be copied.
    Copied { raw: &'b str, parsed: NumberResult },
}

impl JsonNumber<'_, '_> {
    /// Create a JsonNumber::Borrowed from a byte slice.
    ///
    /// This is the main entry point for creating JsonNumber from raw bytes.
    /// It parses the number according to the configured behavior (int8/int32/int64,
    /// float support, etc.) and wraps it in a JsonNumber::Borrowed variant.
    ///
    /// # Arguments
    /// * `bytes` - Raw byte slice containing the JSON number
    ///
    /// # Returns
    /// A JsonNumber::Borrowed with the parsed result, or ParseError if invalid
    pub fn from_slice(bytes: &[u8]) -> Result<JsonNumber<'_, '_>, ParseError> {
        let parsed_result = if is_integer(bytes) {
            parse_integer(bytes)
        } else {
            #[cfg(feature = "float")]
            {
                parse_float(bytes)
            }
            #[cfg(not(feature = "float"))]
            {
                parse_float(bytes)?
            }
        };
        let number_str = crate::shared::from_utf8(bytes)?;
        Ok(JsonNumber::Borrowed {
            raw: number_str,
            parsed: parsed_result,
        })
    }

    /// Get the parsed NumberResult.
    pub fn parsed(&self) -> &NumberResult {
        match self {
            JsonNumber::Borrowed { parsed, .. } => parsed,
            JsonNumber::Copied { parsed, .. } => parsed,
        }
    }

    /// Get the number as the configurable integer type if it's an integer that fits.
    pub fn as_int(&self) -> Option<ConfiguredInt> {
        let parsed = self.parsed();
        match parsed {
            NumberResult::Integer(val) => Some(*val),
            #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
            NumberResult::FloatTruncated(val) => Some(*val),
            _ => None,
        }
    }

    /// Get the number as an f64 if float support is enabled.
    /// For integers, converts to f64. For overflowing integers, returns None.
    #[cfg(feature = "float")]
    pub fn as_f64(&self) -> Option<f64> {
        let parsed = self.parsed();
        match parsed {
            NumberResult::Float(val) => Some(*val),
            NumberResult::Integer(val) => Some(*val as f64),
            _ => None,
        }
    }

    /// Always available: get the exact string representation.
    /// This preserves full precision and never loses information.
    pub fn as_str(&self) -> &str {
        match self {
            JsonNumber::Borrowed { raw, .. } => raw,
            JsonNumber::Copied { raw, .. } => raw,
        }
    }

    /// Parse the number as a custom type using the exact string representation.
    /// This allows using external libraries like BigDecimal, arbitrary precision, etc.
    pub fn parse<T: FromStr>(&self) -> Result<T, T::Err> {
        T::from_str(self.as_str())
    }

    /// Check if this number represents an integer (no decimal point or exponent).
    pub fn is_integer(&self) -> bool {
        let parsed = self.parsed();
        matches!(
            parsed,
            NumberResult::Integer(_) | NumberResult::IntegerOverflow
        )
    }

    /// Returns true if this number is not an integer (i.e., has a decimal point or exponent).
    ///
    /// Note: This does not guarantee that float values are supported or enabled in this build.
    /// It only indicates that the number is not an integer, regardless of float support.
    pub fn is_float(&self) -> bool {
        !self.is_integer()
    }
}

impl AsRef<str> for JsonNumber<'_, '_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for JsonNumber<'_, '_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl core::fmt::Display for JsonNumber<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Display strategy: Show parsed value when available, fall back to raw string
        // This provides the most meaningful representation across all configurations
        let (raw, parsed) = match self {
            JsonNumber::Borrowed { raw, parsed } => (raw, parsed),
            JsonNumber::Copied { raw, parsed } => (raw, parsed),
        };
        match parsed {
            NumberResult::Integer(val) => write!(f, "{val}"),
            #[cfg(feature = "float")]
            NumberResult::Float(val) => write!(f, "{val}"),
            #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
            NumberResult::FloatTruncated(val) => write!(f, "{}", val),
            // For overflow, disabled, or skipped cases, show the exact raw string
            // This preserves full precision and is least surprising to users
            _ => f.write_str(raw),
        }
    }
}

/// Detects if a number byte slice represents an integer (no decimal point or exponent).
/// JSON numbers are pure ASCII, so this avoids unnecessary UTF-8 string processing.
pub fn is_integer(bytes: &[u8]) -> bool {
    for &b in bytes {
        if b == b'.' || b == b'e' || b == b'E' {
            return false;
        }
    }
    true
}

/// Parses an integer byte slice into NumberResult using configured integer type.
/// JSON numbers are pure ASCII, so this avoids unnecessary UTF-8 string processing.
pub const fn parse_integer(bytes: &[u8]) -> NumberResult {
    #[cfg(feature = "int8")]
    let result = from_ascii_i8(bytes);
    #[cfg(feature = "int32")]
    let result = from_ascii_i32(bytes);
    #[cfg(feature = "int64")]
    let result = from_ascii_i64(bytes);

    match result {
        Ok(val) => NumberResult::Integer(val),
        Err(_) => NumberResult::IntegerOverflow,
    }
}

/// Parses a float byte slice into NumberResult (only available with float feature).
/// JSON numbers are pure ASCII, so this avoids unnecessary UTF-8 string processing.
#[cfg(feature = "float")]
pub fn parse_float(bytes: &[u8]) -> NumberResult {
    // Convert bytes to str - JSON numbers are guaranteed ASCII
    let s = match crate::shared::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return NumberResult::IntegerOverflow, // Invalid UTF-8 means invalid number
    };
    match f64::from_str(s) {
        Ok(val) if val.is_finite() => NumberResult::Float(val),
        _ => NumberResult::IntegerOverflow, // Infinity/NaN -> treat as overflow, use raw string
    }
}

/// Parses a float byte slice when float feature is disabled - behavior depends on configuration.
/// JSON numbers are pure ASCII, so this avoids unnecessary UTF-8 string processing.
#[cfg(not(feature = "float"))]
pub fn parse_float(bytes: &[u8]) -> Result<NumberResult, ParseError> {
    // Convert bytes to str - JSON numbers are guaranteed ASCII
    let s = match crate::shared::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Err(ParseError::InvalidNumber), // Invalid UTF-8 means invalid number
    };
    #[cfg(feature = "float-error")]
    {
        let _ = s; // Acknowledge parameter usage
        Err(ParseError::FloatNotAllowed)
    }
    #[cfg(feature = "float-skip")]
    {
        let _ = s; // Acknowledge parameter usage
        Ok(NumberResult::FloatSkipped)
    }
    #[cfg(feature = "float-truncate")]
    {
        // Scientific notation (1e3, 2.5e-1) would require float math to evaluate properly.
        // For embedded targets avoiding float math, we error on scientific notation.
        if s.contains(['e', 'E']) {
            return Err(ParseError::InvalidNumber);
        }

        // Extract integer part before decimal point for simple decimals like 1.5 → 1
        let int_part = if let Some(dot_pos) = s.find('.') {
            s.get(..dot_pos).unwrap_or(s)
        } else {
            s // Should not happen since we detected it's a float, but handle gracefully
        };

        match ConfiguredInt::from_str(int_part) {
            Ok(val) => Ok(NumberResult::FloatTruncated(val)),
            Err(_) => Ok(NumberResult::IntegerOverflow),
        }
    }
    #[cfg(not(any(
        feature = "float-error",
        feature = "float-skip",
        feature = "float-truncate"
    )))]
    {
        let _ = s; // Acknowledge parameter usage
        Ok(NumberResult::FloatDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_number_from_slice_basic() {
        // Test number parsing from byte slices
        let json_number = JsonNumber::from_slice(b"56").unwrap();
        assert_eq!(json_number.as_str(), "56");
        assert_eq!(json_number.as_int(), Some(56));

        let json_number = JsonNumber::from_slice(b"89").unwrap();
        assert_eq!(json_number.as_str(), "89");
        assert_eq!(json_number.as_int(), Some(89));

        // Test with invalid UTF-8 to trigger an error
        let invalid_utf8 = &[0xFF, 0xFE, 0xFD]; // Invalid UTF-8 sequence
        let result = JsonNumber::from_slice(invalid_utf8);
        assert!(result.is_err());
    }

    #[test]
    fn test_json_number_integer() {
        let number = JsonNumber::Borrowed {
            raw: "42",
            parsed: NumberResult::Integer(42),
        };
        assert_eq!(number.as_str(), "42");
        assert_eq!(number.as_int(), Some(42));
        assert!(number.is_integer());
        assert!(!number.is_float());
    }

    #[test]
    fn test_json_number_negative_integer() {
        let number = JsonNumber::Borrowed {
            raw: "-123",
            parsed: NumberResult::Integer(-123),
        };
        assert_eq!(number.as_str(), "-123");
        assert_eq!(number.as_int(), Some(-123));
        assert!(number.is_integer());
    }

    #[test]
    fn test_json_number_large_integer() {
        let large_int_str = "12345678901234567890"; // Larger than configured integer max
        let number = JsonNumber::Borrowed {
            raw: large_int_str,
            parsed: NumberResult::IntegerOverflow,
        };
        assert_eq!(number.as_str(), large_int_str);
        assert_eq!(number.as_int(), None); // Should be None due to overflow
        match number {
            JsonNumber::Borrowed {
                parsed: NumberResult::IntegerOverflow,
                ..
            } => {}
            _ => panic!("Expected IntegerOverflow"),
        }
        assert!(number.is_integer());
    }

    #[test]
    #[cfg(feature = "float")]
    fn test_json_number_float() {
        let number = JsonNumber::Borrowed {
            raw: "3.25",
            parsed: NumberResult::Float(3.25),
        };
        assert_eq!(number.as_str(), "3.25");
        assert_eq!(number.as_int(), None);
        assert_eq!(number.as_f64(), Some(3.25));
        assert!(!number.is_integer());
        assert!(number.is_float());
    }

    #[test]
    #[cfg(feature = "float")]
    fn test_json_number_exponent() {
        let number = JsonNumber::Borrowed {
            raw: "1.5e10",
            parsed: NumberResult::Float(1.5e10),
        };
        assert_eq!(number.as_str(), "1.5e10");
        assert_eq!(number.as_f64(), Some(1.5e10));
        assert!(number.is_float());
    }

    #[test]
    #[cfg(not(feature = "float"))]
    fn test_json_number_float_disabled() {
        let number = JsonNumber::Borrowed {
            raw: "3.14159",
            parsed: NumberResult::FloatDisabled,
        };
        assert_eq!(number.as_str(), "3.14159");
        assert_eq!(number.as_int(), None);
        match number {
            JsonNumber::Borrowed {
                parsed: NumberResult::FloatDisabled,
                ..
            } => {}
            _ => panic!("Expected FloatDisabled"),
        }
        assert!(number.is_float());
    }

    #[test]
    fn test_json_number_parse_custom() {
        let number = JsonNumber::Borrowed {
            raw: "42",
            parsed: NumberResult::Integer(42),
        };
        let parsed: u32 = number.parse().unwrap();
        assert_eq!(parsed, 42u32);

        let float_number = JsonNumber::Borrowed {
            raw: "3.14",
            parsed: NumberResult::Integer(3), // Mock for test, would be Float in real usage
        };
        let parsed_f32: Result<f32, _> = float_number.parse();
        assert!(parsed_f32.is_ok());
    }

    #[test]
    fn test_is_integer_detection() {
        assert!(is_integer("42".as_bytes()));
        assert!(is_integer("-123".as_bytes()));
        assert!(is_integer("0".as_bytes()));
        assert!(!is_integer("3.14".as_bytes()));
        assert!(!is_integer("1e10".as_bytes()));
        assert!(!is_integer("2.5E-3".as_bytes()));
    }

    #[test]
    fn test_from_slice_with_container() {
        // Test parsing number followed by container end delimiter
        let data = b"56}"; // Number followed by container end
        let number_bytes = &data[0..2]; // Extract just "56"

        let json_number = JsonNumber::from_slice(number_bytes).unwrap();
        assert_eq!(json_number.as_str(), "56"); // Should exclude the '}'
        assert_eq!(json_number.as_int(), Some(56));
    }

    #[test]
    fn test_from_slice_at_eof() {
        // Test parsing number at end of data
        let data = b"89";
        let number_bytes = &data[0..2]; // Full data

        let json_number = JsonNumber::from_slice(number_bytes).unwrap();
        assert_eq!(json_number.as_str(), "89"); // Should include full number
        assert_eq!(json_number.as_int(), Some(89));
    }
}
