use std::{fmt, str::FromStr};

use base64::Engine;
use rand::RngExt;

/// A 128 bit session id type representing a base64 16 char string
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sid([u8; 16]);

impl Sid {
    /// A zeroed session id
    pub const ZERO: Self = Self([0u8; 16]);
    /// Generate a new random session id (base64 16 chars)
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the session id as a base64 16 chars string
    pub const fn as_str(&self) -> &str {
        // SAFETY: SID is always a base64 chars string
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

/// Error type for [`Sid::from_str`]
#[derive(Debug)]
pub enum SidDecodeError {
    /// Invalid base64 string
    InvalidBase64String,
    /// Invalid length
    InvalidLength,
}
impl fmt::Display for SidDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SidDecodeError::InvalidBase64String => write!(f, "Invalid url base64 string"),
            SidDecodeError::InvalidLength => write!(f, "Invalid sid length"),
        }
    }
}
impl std::error::Error for SidDecodeError {}

impl FromStr for Sid {
    type Err = SidDecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use SidDecodeError::*;

        let mut id = [0u8; 16];

        // Verify the length of the string
        if s.len() != 16 {
            return Err(InvalidLength);
        }

        // Verify that the string is a valid base64 url safe string without padding
        for (idx, byte) in s.as_bytes()[0..16].iter().enumerate() {
            if byte.is_ascii_alphanumeric() || byte == &b'_' || byte == &b'-' {
                id[idx] = *byte;
            } else {
                return Err(InvalidBase64String);
            }
        }
        Ok(Sid(id))
    }
}

impl Default for Sid {
    fn default() -> Self {
        let mut random = [0u8; 12]; // 12 bytes = 16 chars base64
        let mut id = [0u8; 16];

        rand::rng().fill(&mut random);

        base64::prelude::BASE64_URL_SAFE_NO_PAD
            .encode_slice(random, &mut id)
            .unwrap();

        Sid(id)
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
impl serde::Serialize for Sid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

struct SidVisitor;
impl serde::de::Visitor<'_> for SidVisitor {
    type Value = Sid;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid sid")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Sid::from_str(v).map_err(serde::de::Error::custom)
    }
}
impl<'de> serde::Deserialize<'de> for Sid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(SidVisitor)
    }
}

impl fmt::Debug for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::sid::Sid;

    #[test]
    fn test_sid_from_str() {
        let id = Sid::new();
        let id2 = Sid::from_str(&id.to_string()).unwrap();
        assert_eq!(id, id2);
        let id = Sid::from_str("AA9AAA0AAzAAAAHs").unwrap();
        assert_eq!(id.to_string(), "AA9AAA0AAzAAAAHs");
    }

    #[test]
    fn test_sid_from_str_invalid() {
        let id = Sid::from_str("*$^ùù!").unwrap_err();
        assert_eq!(id.to_string(), "Invalid sid length");
        let id = Sid::from_str("aoassaAZDoin#zd{").unwrap_err();
        assert_eq!(id.to_string(), "Invalid url base64 string");
        let id = Sid::from_str("aoassaAZDoinazd<").unwrap_err();
        assert_eq!(id.to_string(), "Invalid url base64 string");
    }
}
