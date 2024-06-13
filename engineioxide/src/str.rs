use std::borrow::Cow;

use bytes::Bytes;

/// A custom [`Bytes`] wrapper to efficiently store string packets
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub struct Str(Bytes);
impl Str {
    /// Efficiently slice string by calling [`Bytes::slice`] on the inner bytes
    pub fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        Str(self.0.slice(range))
    }
    /// Return a &str representation of the string
    pub fn as_str(&self) -> &str {
        // SAFETY: Str is always a valid utf8 string
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
    /// Return a &[u8] representation of the string
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Get the byte at the specified index
    pub fn get(&self, index: usize) -> Option<&u8> {
        self.0.get(index)
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
