#![warn(
    clippy::all,
    clippy::todo,
    clippy::empty_enum,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]

//! This crate is the core of the socketioxide crate.
//! It contains basic types and interfaces for the socketioxide crate and the parser sub-crates.

pub mod adapter;
pub mod errors;
pub mod packet;
pub mod parser;

use std::{collections::VecDeque, ops::Deref, str::FromStr};

use bytes::Bytes;
pub use engineioxide_core::{Sid, Str};
use serde::{Deserialize, Serialize};

/// Represents a unique identifier for a server.
#[derive(Clone, Serialize, Deserialize, Debug, Copy, PartialEq, Eq, Default)]
pub struct Uid(Sid);
impl Deref for Uid {
    type Target = Sid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::fmt::Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for Uid {
    type Err = <Sid as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Sid::from_str(s)?))
    }
}
impl Uid {
    /// A zeroed server id.
    pub const ZERO: Self = Self(Sid::ZERO);
    /// Create a new unique identifier.
    pub fn new() -> Self {
        Self(Sid::new())
    }
}

/// Represents a value that can be sent over the engine.io wire as an engine.io packet
/// or the data that can be outputed by a binary parser (e.g. [`MsgPackParser`](../socketioxide_parser_msgpack/index.html))
/// or a string parser (e.g. [`CommonParser`](../socketioxide_parser_common/index.html))).
///
/// If you want to deserialize this value to a specific type. You should manually call the `Data` extractor.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A string payload that will be sent as a string engine.io packet.
    /// It can also contain adjacent binary payloads.
    Str(Str, Option<VecDeque<bytes::Bytes>>),
    /// A binary payload that will be sent as a binary engine.io packet
    Bytes(bytes::Bytes),
}

/// Custom implementation to serialize enum variant as u8.
impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            Value::Str(data, bins) => (0u8, data.as_bytes(), bins),
            Value::Bytes(data) => (1u8, data.as_ref(), &None),
        };
        raw.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (idx, data, bins): (u8, Vec<u8>, Option<VecDeque<Bytes>>) =
            Deserialize::deserialize(deserializer)?;
        let res = match idx {
            0 => Value::Str(
                Str::from(String::from_utf8(data).map_err(serde::de::Error::custom)?),
                bins,
            ),
            1 => Value::Bytes(Bytes::from(data)),
            i => Err(serde::de::Error::custom(format!(
                "invalid value type: {}",
                i
            )))?,
        };
        Ok(res)
    }
}

#[cfg(fuzzing)]
#[doc(hidden)]
impl arbitrary::Arbitrary<'_> for Value {
    fn arbitrary(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<Self> {
        let res = match u.arbitrary::<bool>()? {
            true => Value::Bytes(u.arbitrary::<Vec<u8>>()?.into()),
            false => Value::Str(
                u.arbitrary::<String>()?.into(),
                Some(
                    u.arbitrary_iter::<Vec<u8>>()?
                        .filter_map(|b| b.ok().map(bytes::Bytes::from))
                        .collect(),
                ),
            ),
        };
        Ok(res)
    }
}

impl Value {
    /// Convert the value to a str slice if it can or return None
    pub fn as_str(&self) -> Option<&Str> {
        match self {
            Value::Str(data, _) => Some(data),
            Value::Bytes(_) => None,
        }
    }
    /// Convert the value to a [`bytes::Bytes`] instance if it can or return None
    pub fn as_bytes(&self) -> Option<&bytes::Bytes> {
        match self {
            Value::Str(_, _) => None,
            Value::Bytes(data) => Some(data),
        }
    }
    /// Get the length of the value
    pub fn len(&self) -> usize {
        match self {
            Value::Str(data, _) => data.len(),
            Value::Bytes(data) => data.len(),
        }
    }
    /// Check if the value is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Str, Value};
    use bytes::Bytes;
    use std::collections::VecDeque;

    fn assert_serde_value(value: Value) {
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: Value = serde_json::from_str(&serialized).unwrap();

        assert_eq!(value, deserialized);
    }

    #[test]
    fn value_serde_str_with_bins() {
        let mut bins = VecDeque::new();
        bins.push_back(Bytes::from_static(&[1, 2, 3, 4]));
        bins.push_back(Bytes::from_static(&[5, 6, 7, 8]));

        let value = Value::Str(Str::from("value".to_string()), Some(bins));
        assert_serde_value(value);
    }

    #[test]
    fn value_serde_bytes() {
        let value = Value::Bytes(Bytes::from_static(&[1, 2, 3, 4]));
        assert_serde_value(value);
    }

    #[test]
    fn value_serde_str_without_bins() {
        let value = Value::Str(Str::from("value_no_bins".to_string()), None);
        assert_serde_value(value);
    }

    #[test]
    fn value_serde_invalid_type() {
        let invalid_data = "[2, [1,2,3,4], null]";
        let result: Result<Value, _> = serde_json::from_str(invalid_data);
        assert!(result.is_err());
    }
}
