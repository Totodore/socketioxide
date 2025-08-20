#![cfg_attr(docsrs, feature(doc_cfg))]
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
    rust_2024_compatibility,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]
#![doc = include_str!("../Readme.md")]

pub use engineioxide_core::{ProtocolVersion, Str, TransportType};
pub use socket::{DisconnectReason, Socket};

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
pub use engineioxide_core::{OpenPacket, Packet, PacketParseError};

pub mod config;
pub mod handler;
pub mod layer;
pub mod service;
pub mod socket;

/// Socket id type and generator
pub mod sid {
    #[deprecated(since = "0.16.2", note = "Use engineioxide::socket::Sid instead")]
    pub use engineioxide_core::Sid;
}

mod body;
mod engine;
mod errors;
mod transport;
