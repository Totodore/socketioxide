#![allow(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/Totodore/socketioxide/refs/heads/main/.github/logo_dark.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/Totodore/socketioxide/refs/heads/main/.github/logo_dark.ico"
)]
//! Engine.IO client library for Rust.

mod client;
mod event;
mod transport;

pub use crate::client::Client;
pub use crate::event::EioEvent;
pub use crate::transport::PollingTransport;
pub use transport::{WsTransport, noop_impl, tungstenite_impl};
