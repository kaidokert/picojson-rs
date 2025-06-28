use core::ops::Deref;
use core::str::FromStr;

use crate::ParseError;

// Type alias for the configured integer type
#[cfg(feature = "int32")]
type ConfiguredInt = i32;
#[cfg(not(feature = "int32"))]
type ConfiguredInt = i64;

/// Represents the parsed result of a JSON number.
#[derive(Debug, PartialEq)]
pub enum NumberResult {
    /// Integer that fits in the configured integer type
    Integer(ConfiguredInt),
    /// Integer too large for configured type (use raw string for exact representation)
    IntegerOverflow,
    /// Float value (only available with float feature)
    #[cfg(feature = "float")]
    Float(f64),
    /// Float parsing disabled - behavior depends on configuration
    #[cfg(not(feature = "float"))]
    FloatDisabled,
    /// Float encountered but skipped due to float-skip configuration
    #[cfg(all(not(feature = "float"), feature = "float-skip"))]
    FloatSkipped,
    /// Float truncated to integer due to float-truncate configuration
    #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
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

impl<'a, 'b> JsonNumber<'a, 'b> {
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

    /// Check if this number would be a float (has decimal point or exponent).
    pub fn is_float(&self) -> bool {
        !self.is_integer()
    }
}

impl<'a, 'b> AsRef<str> for JsonNumber<'a, 'b> {
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

impl<'a, 'b> core::fmt::Display for JsonNumber<'a, 'b> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Display strategy: Show parsed value when available, fall back to raw string
        // This provides the most meaningful representation across all configurations
        let (raw, parsed) = match self {
            JsonNumber::Borrowed { raw, parsed } => (raw, parsed),
            JsonNumber::Copied { raw, parsed } => (raw, parsed),
        };
        match parsed {
            NumberResult::Integer(val) => write!(f, "{}", val),
            #[cfg(feature = "float")]
            NumberResult::Float(val) => write!(f, "{}", val),
            #[cfg(all(not(feature = "float"), feature = "float-truncate"))]
            NumberResult::FloatTruncated(val) => write!(f, "{}", val),
            // For overflow, disabled, or skipped cases, show the exact raw string
            // This preserves full precision and is least surprising to users
            _ => f.write_str(raw),
        }
    }
}

/// Detects if a number string represents an integer (no decimal point or exponent).
pub(super) fn is_integer(s: &str) -> bool {
    !s.contains('.') && !s.contains('e') && !s.contains('E')
}

/// Parses an integer string into NumberResult using configured integer type.
pub(super) fn parse_integer(s: &str) -> NumberResult {
    match ConfiguredInt::from_str(s) {
        Ok(val) => NumberResult::Integer(val),
        Err(_) => NumberResult::IntegerOverflow,
    }
}

/// Parses a float string into NumberResult (only available with float feature).
#[cfg(feature = "float")]
pub(super) fn parse_float(s: &str) -> NumberResult {
    match f64::from_str(s) {
        Ok(val) if val.is_finite() => NumberResult::Float(val),
        _ => NumberResult::IntegerOverflow, // Infinity/NaN -> treat as overflow, use raw string
    }
}

/// Parses a float string when float feature is disabled - behavior depends on configuration.
#[cfg(not(feature = "float"))]
pub(super) fn parse_float(_s: &str) -> Result<NumberResult, ParseError> {
    #[cfg(feature = "float-error")]
    {
        Err(ParseError::FloatNotAllowed)
    }
    #[cfg(feature = "float-skip")]
    {
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
            &s[..dot_pos]
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
        Ok(NumberResult::FloatDisabled)
    }
}

/// Parses a JSON number from a string slice.
///
/// This is the main entry point for parsing numbers with all the configured
/// behavior (int32/int64, float support, etc.).
pub(super) fn parse_number_from_str(s: &str) -> Result<NumberResult, ParseError> {
    if is_integer(s) {
        Ok(parse_integer(s))
    } else {
        #[cfg(feature = "float")]
        {
            Ok(parse_float(s))
        }
        #[cfg(not(feature = "float"))]
        {
            parse_float(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            raw: "3.14159",
            parsed: NumberResult::Float(3.14159),
        };
        assert_eq!(number.as_str(), "3.14159");
        assert_eq!(number.as_int(), None);
        assert_eq!(number.as_f64(), Some(3.14159));
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
        assert!(is_integer("42"));
        assert!(is_integer("-123"));
        assert!(is_integer("0"));
        assert!(!is_integer("3.14"));
        assert!(!is_integer("1e10"));
        assert!(!is_integer("2.5E-3"));
    }
}
