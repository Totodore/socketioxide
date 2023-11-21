#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(
    clippy::future_not_send,
    clippy::all,
    clippy::todo,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::mismatched_target_os,
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
    clippy::str_to_string,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]
//! Socketioxide is a socket.io server implementation that works as a [`tower`] layer/service so it integrates nicely with the rest of the ecosystem.
//!
//! ## Table of contents
//! * [Features](#features)
//! * [Compatibility](#compatibility)
//! * [Usage](#usage)
//! * [Handlers](#handlers)
//! * [Extractors](#extractors)
//! * [Events](#events)
//! * [Emiting data](#emiting-data)
//! * [Acknowledgements](#acknowledgements)
//! * [Feature flags](#feature-flags)
//!
//! ## Features
//! * Easy to use flexible axum-like API
//! * Fully compatible with the official [socket.io client](https://socket.io/docs/v4/client-api/)
//! * Support for the previous version of the protocol (v4).
//! * Namespaces
//! * Rooms
//! * Acknowledgements
//! * Polling & Websocket transports
//!
//! ## Compatibility
//! Because it works as a tower [`layer`](tower::layer)/[`service`](tower::Service) you can use it with any http server framework that works with tower/hyper:
//! * [Axum](https://docs.rs/axum/latest/axum/)
//! * [Warp](https://docs.rs/warp/latest/warp/)
//! * [Hyper](https://docs.rs/hyper/latest/hyper/)
//! * [Salvo](https://docs.rs/salvo/latest/salvo/)
//!
//! Note that for v1 of `hyper` and salvo, you need to enable the `hyper-v1` feature and call `with_hyper_v1` on your layer/service.
//!
//! Check the [examples](http://github.com/totodore/socketioxide/tree/main/examples) for more details on framework integration.
//!
//! ## Usage
//! The API tries to mimic the equivalent JS API as much as possible. The main difference is that the default namespace `/` is not created automatically, you need to create it manually.
//!
//! #### Basic example with axum:
//! ```no_run
//! use axum::routing::get;
//! use axum::Server;
//! use socketioxide::{
//!     extract::SocketRef,
//!     SocketIo,
//! };
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let (layer, io) = SocketIo::new_layer();
//!
//!     // Register a handler for the default namespace
//!     io.ns("/", |s: SocketRef| {
//!         // For each "message" event received, send a "message-back" event with the same data
//!         socket.on("message", |socket: SocketRef| {
//!             socket.emit("message-back", data).ok();
//!         });
//!     });
//!
//!     let app = axum::Router::new()
//!         .route("/", get(|| async { "Hello, World!" }))
//!         .layer(layer);
//!
//!     Server::bind(&"127.0.0.1:3000".parse().unwrap())
//!         .serve(app.into_make_service())
//!         .await?;
//!
//!     Ok(())
//! }
//! ```
//! ## Handlers
//! Handlers are functions or clonable closures that are given to the `io.ns` and the `socket.on` methods. They can be async or sync and can take from 0 to 16 arguments that implements the [`FromConnectParts`](handler::FromConnectParts) trait for the [`ConnectHandler`](handler::ConnectHandler) and the [`FromMessageParts`](handler::FromMessageParts) for the [`MessageHandler`](handler::MessageHandler). They are greatly inspired by the axum handlers.
//!
//! If they are async, a new task will be spawned for each incoming connection/message so it doesn't block the event management task.
//!
//! Check the [`handler::connect`] module doc for more details on the connect handler
//! Check the [`handler::message`] module doc for more details on the message handler.
//!
//! ## Extractors
//! Handlers params are called extractors and are used to extract data from the incoming connection/message. They are inspired by the axum extractors.
//! An extractor is a struct that implements the [`FromConnectParts`](handler::FromConnectParts) trait for the [`ConnectHandler`](handler::ConnectHandler) and the [`FromMessageParts`](handler::FromMessageParts) for the [`MessageHandler`](handler::MessageHandler).
//!
//! Here are some examples of extractors:
//! * [`Data`](extract::Data): extracts and deserialize to json any data, if a deserialize error occurs the handler won't be called
//!     - for [`ConnectHandler`](handler::ConnectHandler): extracts and deserialize to json the auth data
//!     - for [`MessageHandler`](handler::MessageHandler): extracts and deserialize to json the message data
//! * [`TryData`](extract::Data): extracts and deserialize to json any data but with a `Result` type in case of error
//!     - for [`ConnectHandler`](handler::ConnectHandler): extracts and deserialize to json the auth data
//!     - for [`MessageHandler`](handler::MessageHandler): extracts and deserialize to json the message data
//! * [`SocketRef`](extract::Data): extracts a reference to the [`Socket`](socket::Socket)
//! * [`Bin`](extract::Data): extract a binary payload for a given message. Because it consumes the event it should be the last argument
//! * [`AckSender`](extract::Data): Can be used to send an ack response to the current message event
//!
//! ### Extractor order
//! Extractors are run in the order of their declaration in the handler signature. If an extractor returns an error, the handler won't be called and a `tracing::error!` call will be emitted if the `tracing` feature is enabled.
//!
//! For the [`MessageHandler`](handler::MessageHandler) some extractors require to _consume_ the event and therefore only implement the [`FromMessage`](handler::FromMessage) trait, like the [`Bin`](extract::Bin) extractor, therefore they should be the last argument.
//!
//! Note that any extractor that implements the [`FromMessageParts`](handler::FromMessageParts) also implements by default the [`FromMessage`](handler::FromMessage) trait.
//!
//! ## Events
//! There are three types of events:
//! * The connect event is emitted when a new connection is established. It can be handled with the [`ConnectHandler`](handler::ConnectHandler) and the `io.ns` method.
//! * The message event is emitted when a new message is received. It can be handled with the [`MessageHandler`](handler::MessageHandler) and the `socket.on` method.
//! * The disconnect event is emitted when a socket is closed. Contrary to the two previous events, the callback is not flexible, it *has* to be async and have the following signature `async fn(SocketRef, DisconnectReason)`. It can be handled with the `socket.on_disconnect` method.
//!
//! Only one handler can exist for an event so registering a new handler for an event will replace the previous one.
//!
//! ## Emiting data
//! Data can be emitted to a socket with the [`Socket::emit`](socket::Socket::emit) method. It takes a name and a data argument. The data argument can be any type that implements the [`serde::Serialize`](serde::Serialize) trait.
pub mod adapter;

#[cfg_attr(docsrs, doc(cfg(feature = "extensions")))]
#[cfg(feature = "extensions")]
pub mod extensions;
#[cfg_attr(docsrs, doc(cfg(feature = "hyper-v1")))]
#[cfg(feature = "hyper-v1")]
pub mod hyper_v1;

pub mod handler;
pub mod layer;
pub mod operators;
pub mod service;
pub mod socket;

#[cfg(feature = "test-utils")]
pub use packet::*;

pub use engineioxide::TransportType;
pub use errors::{AckError, BroadcastError, SendError};
pub use handler::extract;
pub use io::{SocketIo, SocketIoBuilder, SocketIoConfig};

mod client;
mod errors;
mod io;
mod ns;
mod packet;

/// Socket.IO protocol version
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    /// The socket.io protocol version 4, only available with the feature flag `v4`
    V4 = 4,
    /// The socket.io protocol version 5, enabled by default
    V5 = 5,
}

impl From<ProtocolVersion> for engineioxide::ProtocolVersion {
    fn from(value: ProtocolVersion) -> Self {
        match value {
            ProtocolVersion::V4 => Self::V3,
            ProtocolVersion::V5 => Self::V4,
        }
    }
}
impl From<engineioxide::ProtocolVersion> for ProtocolVersion {
    fn from(value: engineioxide::ProtocolVersion) -> Self {
        match value {
            engineioxide::ProtocolVersion::V3 => Self::V4,
            engineioxide::ProtocolVersion::V4 => Self::V5,
        }
    }
}
