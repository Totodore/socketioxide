#![warn(clippy::pedantic)]
#![allow(clippy::similar_names)]
//! Engine.IO client library for Rust.

mod client;
mod io;
mod transport;

pub use crate::transport::polling::PollingClient;
