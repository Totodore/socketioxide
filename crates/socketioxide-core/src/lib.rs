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

use std::collections::VecDeque;

pub use engineioxide::{sid::Sid, Str};

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

#[cfg(feature = "fuzzing")]
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
