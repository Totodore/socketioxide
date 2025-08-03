// #![warn(clippy::pedantic)]
#![allow(clippy::similar_names)]
//! Engine.IO client library for Rust.

mod client;
mod io;
mod transport;
pub use crate::transport::polling::HttpClient;

#[macro_export]
macro_rules! poll {
    ($expr:expr) => {
        match $expr {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(value) => value,
        }
    };
}
