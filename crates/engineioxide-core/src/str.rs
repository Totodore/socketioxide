use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::borrow::{Borrow, Cow};

/// A custom [`Bytes`] wrapper to efficiently store string packets
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd)]
pub struct Str(Bytes);
impl Str {
    /// Efficiently slice string by calling [`Bytes::slice`] on the inner bytes
    pub fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        Str(self.0.slice(range))
    }
    /// Return a `&str` representation of the string
    pub fn as_str(&self) -> &str {
        // SAFETY: Str is always a valid utf8 string
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
    /// Return a `&[u8]` representation of the string
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Get the byte at the specified index
    pub fn get(&self, index: usize) -> Option<&u8> {
        self.0.get(index)
    }
    /// Creates a [`Str`] instance from str slice, by copying it.
    pub fn copy_from_slice(data: &str) -> Self {
        Str(Bytes::copy_from_slice(data.as_bytes()))
    }
    /// Creates a [`Str`] instance from a [`Bytes`] slice. It is the caller's responsibility to
    /// ensure that the provided bytes are a valid utf8 string.
    ///
    /// # Safety
    /// It is the caller's responsibility to ensure that the provided bytes are a valid utf8 string.
    pub unsafe fn from_bytes_unchecked(data: Bytes) -> Self {
        Str(data)
    }
}
/// This custom Hash implementation as a [`str`] is made to match with the [`Borrow`]
/// implementation as [`str`]. Otherwise [`str`] and [`Str`] won't have the same hash.
impl std::hash::Hash for Str {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        str::hash(self.as_str(), state);
    }
}
impl std::ops::Deref for Str {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}
impl std::fmt::Display for Str {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
impl Borrow<str> for Str {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl From<&'static str> for Str {
    fn from(s: &'static str) -> Self {
        Str(Bytes::from_static(s.as_bytes()))
    }
}
impl From<String> for Str {
    fn from(s: String) -> Self {
        let vec = s.into_bytes();
        Str(Bytes::from(vec))
    }
}

impl From<Cow<'static, str>> for Str {
    fn from(s: Cow<'static, str>) -> Self {
        match s {
            Cow::Borrowed(s) => Str::from(s),
            Cow::Owned(s) => Str::from(s),
        }
    }
}
impl From<&Cow<'static, str>> for Str {
    fn from(s: &Cow<'static, str>) -> Self {
        match s {
            Cow::Borrowed(s) => Str::from(*s),
            Cow::Owned(s) => Str(Bytes::copy_from_slice(s.as_bytes())),
        }
    }
}

impl From<Str> for Bytes {
    fn from(s: Str) -> Self {
        s.0
    }
}
impl From<Str> for String {
    fn from(s: Str) -> Self {
        let vec = s.0.into();
        // SAFETY: Str is always a valid utf8 string
        unsafe { String::from_utf8_unchecked(vec) }
    }
}
impl From<Str> for Vec<u8> {
    fn from(value: Str) -> Self {
        Vec::from(value.0)
    }
}
impl Serialize for Str {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for Str {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StrVisitor;
        impl serde::de::Visitor<'_> for StrVisitor {
            type Value = Str;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a str")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Str::copy_from_slice(v))
            }
            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Str::from(v))
            }
        }
        deserializer.deserialize_str(StrVisitor)
    }
}

impl std::cmp::PartialEq<&str> for Str {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
impl std::cmp::PartialEq<Str> for &str {
    fn eq(&self, other: &Str) -> bool {
        *self == other.as_str()
    }
}
