// SPDX-License-Identifier: Apache-2.0

use core::ops::Deref;

/// Represents a JSON string.
///
/// 'a is the lifetime of the original input buffer.
/// 'b is the lifetime of the scratch buffer.
#[derive(Debug, PartialEq, Eq)]
pub enum String<'a, 'b> {
    /// A raw slice from the original input, used when no un-escaping is needed.
    Borrowed(&'a str),
    /// A slice from the scratch buffer, used when a string had to be un-escaped.
    Unescaped(&'b str),
}

impl String<'_, '_> {
    /// Returns the string as a `&str`, whether borrowed or unescaped.
    pub fn as_str(&self) -> &str {
        match self {
            String::Borrowed(s) => s,
            String::Unescaped(s) => s,
        }
    }
}

impl AsRef<str> for String<'_, '_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for String<'_, '_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            String::Borrowed(s) => s,
            String::Unescaped(s) => s,
        }
    }
}

impl<'a> From<&'a str> for String<'a, '_> {
    fn from(s: &'a str) -> Self {
        String::Borrowed(s)
    }
}

impl core::fmt::Display for String<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_string_deref() {
        let borrowed = String::Borrowed("test");
        assert_eq!(&*borrowed, "test");
        assert_eq!(borrowed.len(), 4);

        // Test that it works as a string reference
        fn takes_str(s: &str) -> usize {
            s.len()
        }
        assert_eq!(takes_str(&borrowed), 4);
    }
}
