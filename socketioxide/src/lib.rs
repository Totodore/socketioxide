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
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]
//! Socketioxide is a socket.io server implementation that works as a [`tower`] layer/service.
//! It integrates nicely with the rest of the [`tower`]/[`tokio`]/[`hyper`](https://docs.rs/hyper/latest/hyper/) ecosystem.
//!
//! ## Table of contents
//! * [Features](#features)
//! * [Compatibility](#compatibility)
//! * [Usage](#usage)
//! * [Initialisation](#initialisation)
//! * [Handlers](#handlers)
//! * [Extractors](#extractors)
//! * [Events](#events)
//! * [Emiting data](#emiting-data)
//! * [Acknowledgements](#acknowledgements)
//! * [Adapters](#adapters)
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
//! Because it works as a tower [`layer`](tower::layer)/[`service`](tower::Service) you can use it with any http server frameworks that works with tower/hyper:
//! * [Axum](https://docs.rs/axum/latest/axum/)
//! * [Warp](https://docs.rs/warp/latest/warp/)
//! * [Hyper](https://docs.rs/hyper/latest/hyper/)
//! * [Salvo](https://docs.rs/salvo/latest/salvo/)
//!
//! Note that for v1 of `hyper` and salvo, you need to enable the `hyper-v1` feature and call `with_hyper_v1` on your layer/service.
//!
//! Check the [examples](http://github.com/totodore/socketioxide/tree/main/examples) for more details on frameworks integration.
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
//!         // For each "message" event received, send a "message-back" event with the "Hello World!" event
//!         s.on("message", |s: SocketRef| {
//!             s.emit("message-back", "Hello World!").ok();
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
//! ## Initialisation
//! The [`SocketIo`] struct is the main entry point of the library. It is used to create a [`Layer`](tower::layer) or a [`Service`](tower::Service).
//! Later it can be used as the equivalent of the `io` object in the JS API.
//!
//! When creating your [`SocketIo`] instance, you can use the builder pattern to configure it with the [`SocketIoBuilder`] struct.
//! * See the [`SocketIoBuilder`] doc for more details on the available configuration options.
//! * See the [`layer`] module doc for more details on layers.
//! * See the [`service`] module doc for more details on services.
//! * See the [`hyper_v1`] module doc for more details on hyper v1 compatibility.
//!
//! #### Tower layer example with custom configuration:
//! ```
//! use socketioxide::SocketIo;
//! let (layer, io) = SocketIo::builder()
//!     .max_payload(10_000_000) // Max HTTP payload size of 10M
//!     .max_buffer_size(10_000) // Max number of packets in the buffer
//!     .build_layer();
//! ```
//!
//! #### Tower _standalone_ service example with default configuration:
//! ```
//! use socketioxide::SocketIo;
//! let (svc, io) = SocketIo::new_svc();
//! ```
//!
//! #### Tower service example with hyper v1 (requires the `hyper-v1` feature) and default configuration:
//! ```
//! use socketioxide::SocketIo;
//! let (svc, io) = SocketIo::new_svc();
//! let svc = svc.with_hyper_v1();
//! ```
//! #### Tower layer example with hyper v1 (requires the `hyper-v1` feature) and default configuration:
//! ```
//! use socketioxide::SocketIo;
//! let (layer, io) = SocketIo::new_layer();
//! let layer = layer.with_hyper_v1();
//! ```
//!
//! ## Handlers
//! Handlers are functions or clonable closures that are given to the `io.ns`, the `socket.on` and the `socket.on_disconnect` fns.
//! They can be async or sync and can take from 0 to 16 arguments that implements the [`FromConnectParts`](handler::FromConnectParts)
//! trait for the [`ConnectHandler`](handler::ConnectHandler), the [`FromMessageParts`](handler::FromMessageParts) for
//! the [`MessageHandler`](handler::MessageHandler) and the [`FromDisconnectParts`](handler::FromDisconnectParts) for
//! the [`DisconnectHandler`](handler::DisconnectHandler).
//! They are greatly inspired by the axum handlers.
//!
//! If they are async, a new task will be spawned for each incoming connection/message so it doesn't block the event management task.
//!
//! * Check the [`handler::connect`] module doc for more details on the connect handler
//! * Check the [`handler::message`] module doc for more details on the message handler.
//! * Check the [`handler::disconnect`] module doc for more details on the disconnect handler.
//! * Check the [`handler::extract`] module doc for more details on the extractors.
//!
//! ## Extractors
//! Handlers params are called extractors and are used to extract data from the incoming connection/message. They are inspired by the axum extractors.
//! An extractor is a struct that implements the [`FromConnectParts`](handler::FromConnectParts) trait for the [`ConnectHandler`](handler::ConnectHandler)
//! the [`FromMessageParts`](handler::FromMessageParts) for the [`MessageHandler`](handler::MessageHandler) and the
//! [`FromDisconnectParts`](handler::FromDisconnectParts) for the [`DisconnectHandler`](handler::DisconnectHandler).
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
//! * [`ProtocolVersion`]: extracts the protocol version of the socket
//! * [`TransportType`]: extracts the transport type of the socket
//!
//! ### Extractor order
//! Extractors are run in the order of their declaration in the handler signature. If an extractor returns an error, the handler won't be called and a `tracing::error!` call will be emitted if the `tracing` feature is enabled.
//!
//! For the [`MessageHandler`](handler::MessageHandler), some extractors require to _consume_ the event and therefore only implement the [`FromMessage`](handler::FromMessage) trait, like the [`Bin`](extract::Bin) extractor, therefore they should be the last argument.
//!
//! Note that any extractors that implement the [`FromMessageParts`](handler::FromMessageParts) also implement by default the [`FromMessage`](handler::FromMessage) trait.
//!
//! ## Events
//! There are three types of events:
//! * The connect event is emitted when a new connection is established. It can be handled with the [`ConnectHandler`](handler::ConnectHandler) and the `io.ns` method.
//! * The message event is emitted when a new message is received. It can be handled with the [`MessageHandler`](handler::MessageHandler) and the `socket.on` method.
//! * The disconnect event is emitted when a socket is closed. It can be handled with the [`DisconnectHandler`](handler::DisconnectHandler) and the `socket.on_disconnect` method.
//!
//! Only one handler can exist for an event so registering a new handler for an event will replace the previous one.
//!
//! ## [Emiting data](#emiting-data)
//! Data can be emitted to a socket with the [`Socket::emit`](socket::Socket) method. It takes an event name and a data argument.
//! The data argument can be any type that implements the [`serde::Serialize`] trait.
//!
//! You can emit from the [`SocketIo`] handle or the [`SocketRef`](extract::SocketRef).
//! The difference is that you can move the [`io`](SocketIo) handle everywhere because it is a cheaply cloneable struct.
//! The [`SocketRef`](extract::SocketRef) is a reference to the socket and cannot be cloned.
//!
//! Moreover the [`io`](SocketIo) handle can emit to any namespace while the [`SocketRef`](extract::SocketRef) can only emit to the namespace of the socket.
//!
//! #### Emit errors
//! If the data can't be serialized to json, an [`serde_json::Error`] will be returned.
//! If the socket is disconnected or the internal channel is full, a tracing log will be emitted if the `tracing` feature is enabled and the message will be dropped.
//! This solution is not ideal and will be improved in the future (see [socketioxide/172](https://github.com/Totodore/socketioxide/issues/172)).
//!
//! #### Emitting with operators
//! To configure the emit, you can chain [`Operators`](operators::Operators) methods to the emit call. With that you can easily configure the following options:
//! * rooms: emit, join, leave to specific rooms
//! * namespace: emit to a specific namespace (only from the [`SocketIo`] handle)
//! * timeout: set a custom timeout when waiting for an ack
//! * binary: emit a binary payload with the message
//! * local: broadcast only to the current node (in case of a cluster)
//!
//! Check the [`operators::Operators`] doc for more details on the operators.
//!
//! ## Acknowledgements
//! You can ensure that a message has been received by the client/server with acknowledgements.
//!
//! #### Server acknowledgements
//! They are implemented with the [`AckSender`](extract::AckSender) extractor.
//! You can send an ack response with an optional binary payload with the [`AckSender::send`](extract::AckSender) method.
//! If the client doesn't send an ack response, the [`AckSender::send`](extract::AckSender) method will do nothing.
//!
//! #### Client acknowledgements
//! You can use the [`Socket::emit_with_ack`](socket::Socket) method to emit a message with an ack callback.
//! It will return a [`Future`](futures::Future) that will resolve when the acknowledgement is received.
//!
//! ## Adapters
//! This library is designed to work with clustering. It uses the [`Adapter`](adapter::Adapter) trait to abstract the underlying storage.
//! By default it uses the [`LocalAdapter`](adapter::LocalAdapter) which is a simple in-memory adapter.
//! Currently there is no other adapters available but more will be added in the future.
//!
//! ## [Feature flags](#feature-flags)
//! * `hyper-v1`: enable support for hyper v1
//! * `v4`: enable support for the socket.io protocol v4
//! * `tracing`: enable logging with [`tracing`] calls
//! * `extensions`: enable [`extensions`] module
//!
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

/// Socket.IO protocol version.
/// It is accessible with the [`Socket::protocol`](socket::Socket) method or as an extractor
///
/// **Note**: The socket.io protocol version does not correspond to the engine.io protocol version.
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
