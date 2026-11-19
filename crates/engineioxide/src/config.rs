//! ## Configuration for the engine.io engine & transports
//! #### Example :
//! ```rust
//! # use bytes::Bytes;
//! # use engineioxide::config::EngineIoConfig;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::handler::EngineIoHandler;
//! # use std::time::Duration;
//! # use engineioxide::{Socket, DisconnectReason, Str};
//! # use std::sync::Arc;
//! #[derive(Debug, Clone)]
//! struct MyHandler;
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = ();
//!     fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) { }
//!     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) { }
//!     fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<()>>) { }
//!     fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<()>>) { }
//! }
//!
//! let config = EngineIoConfig::builder()
//!     .ping_interval(Duration::from_millis(300))  // Set the ping_interval to 300ms
//!     .ping_timeout(Duration::from_millis(200))   // Set the ping timeout to 200ms
//!     .max_payload(1e6 as u64)                    // Set the max payload to a given size
//!     .max_buffer_size(1024)                      // Set a custom buffer size
//!     .build();
//!
//! // Create an engine io service with a custom config
//! let svc = EngineIoService::with_config(Arc::new(MyHandler), config);
//! ```

use std::{borrow::Cow, time::Duration};

use engineioxide_core::TransportType;

/// Configuration for the engine.io engine & transports
#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    /// The path to listen for engine.io requests on.
    /// Defaults to "/engine.io".
    pub req_path: Cow<'static, str>,

    /// The interval at which the server will send a ping packet to the client.
    /// Defaults to 25 seconds.
    pub ping_interval: Duration,

    /// The amount of time the server will wait for a ping response from the client before closing the connection.
    /// Defaults to 20 seconds.
    pub ping_timeout: Duration,

    /// The amount of time the server will wait for a transport upgrade to complete before aborting it.
    /// Defaults to 10 seconds.
    pub upgrade_timeout: Duration,

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    ///
    /// If the buffer if full the `emit()` method will return an error
    ///
    /// Defaults to 128 packets
    pub max_buffer_size: usize,

    /// The maximum number of bytes that can be received per http request.
    /// Defaults to 100KB.
    pub max_payload: u64,

    /// The size of the read buffer for the websocket transport.
    /// You can tweak this value depending on your use case. By default it is set to 4KiB.
    ///
    /// Setting it to a higher value will improve performance on heavy read scenarios
    /// but will consume more memory.
    pub ws_read_buffer_size: usize,

    /// The maximum size of an incoming message
    ///
    /// The default value is 64 MiB which should be reasonably big for all normal use-cases but small enough
    /// to prevent memory eating by a malicious user.
    pub ws_max_message_size: usize,

    /// The maximum size of a single incoming message frame.
    /// The limit is for frame payload NOT including the frame header.
    ///
    /// The default value is 16 MiB which should be reasonably big for all normal use-cases but small enough
    /// to prevent memory eating by a malicious user.
    pub ws_max_frame_size: usize,

    /// Allowed transports on this server
    /// It is represented as a bitfield to allow to combine any number of transports easily
    pub transports: u8,

    /// The configuration of the `permessage-deflate` websocket extension.
    /// The extension is not offered to the clients if this is `None`.
    ///
    /// Defaults to `None`.
    #[cfg(feature = "ws-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ws-deflate")))]
    pub ws_per_message_deflate: Option<PerMessageDeflateConfig>,
}

/// Configuration of the `permessage-deflate` websocket extension ([RFC 7692]).
///
/// Compression trades bandwidth for CPU and memory: with the default settings, each connection
/// that compresses messages holds around 370 KiB for its compression context, and each one that
/// receives compressed messages holds around 45 KiB for its decompression context. This is allocated
/// on the first compressed message of each direction, so a connection that doesn't need it
/// doesn't pay for it.
///
/// To reduce this memory usage, the `no_context_takeover` options free these contexts between
/// two messages, and [`server_max_window_bits`](Self::server_max_window_bits) shrinks the
/// compression context.
///
/// A message is compressed per connection, so broadcasting a message to `n` sockets
/// compresses it `n` times.
///
/// # Example
/// ```
/// # use engineioxide::config::{EngineIoConfig, PerMessageDeflateConfig};
/// let config = EngineIoConfig::builder()
///     .ws_per_message_deflate(
///         PerMessageDeflateConfig::default()
///             .threshold(2048)
///             .level(3)
///             .server_no_context_takeover(true)
///             .client_no_context_takeover(true)
///             .server_max_window_bits(12)
///             .client_max_window_bits(12),
///     )
///     .build();
/// ```
///
/// [RFC 7692]: https://www.rfc-editor.org/rfc/rfc7692
#[cfg(feature = "ws-deflate")]
#[cfg_attr(docsrs, doc(cfg(feature = "ws-deflate")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerMessageDeflateConfig {
    pub(crate) threshold: usize,
    pub(crate) level: u32,
    pub(crate) server_no_context_takeover: bool,
    pub(crate) client_no_context_takeover: bool,
    pub(crate) server_max_window_bits: u8,
    pub(crate) client_max_window_bits: u8,
}

#[cfg(feature = "ws-deflate")]
impl Default for PerMessageDeflateConfig {
    fn default() -> Self {
        Self {
            threshold: 1024,
            level: 6,
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            server_max_window_bits: 15,
            client_max_window_bits: 15,
        }
    }
}

#[cfg(feature = "ws-deflate")]
impl PerMessageDeflateConfig {
    /// The minimum size in bytes of an outgoing message to compress it.
    /// Compressing a small message is often not worth the CPU time and can even make it larger.
    ///
    /// Defaults to 1 KiB.
    pub fn threshold(mut self, threshold: usize) -> Self {
        self.threshold = threshold;
        self
    }

    /// The compression level of the outgoing messages, from 0 (no compression) to 9
    /// (best compression but slowest). A higher value is treated as 9.
    ///
    /// Defaults to 6.
    pub fn level(mut self, level: u32) -> Self {
        self.level = level.min(9);
        self
    }

    /// Don't reuse the compression context between two outgoing messages. This frees the memory
    /// of the context between two messages, but compresses less and costs an allocation per message.
    /// This is enforced if the client asks for it.
    ///
    /// Defaults to `false`.
    pub fn server_no_context_takeover(mut self, server_no_context_takeover: bool) -> Self {
        self.server_no_context_takeover = server_no_context_takeover;
        self
    }

    /// Require the client to not reuse its compression context between two messages. This frees the
    /// memory of the decompression context between two messages, but the client compresses less
    /// and this costs an allocation per message.
    ///
    /// Defaults to `false`.
    pub fn client_no_context_takeover(mut self, client_no_context_takeover: bool) -> Self {
        self.client_no_context_takeover = client_no_context_takeover;
        self
    }

    /// The base-2 logarithm of the size of the window used to compress the outgoing messages,
    /// from 9 (512 bytes) to 15 (32 KiB). A value out of this range is brought back to it.
    /// A smaller window uses less memory but compresses less: the compression context takes
    /// around 370 KiB with a value of 15 and 260 KiB with a value of 12. A lower value than this
    /// doesn't save much more.
    ///
    /// A client can ask for a smaller window than this one, except for a window of 8 bits
    /// which isn't supported: its offer is then declined and the connection isn't compressed.
    ///
    /// Defaults to 15.
    pub fn server_max_window_bits(mut self, server_max_window_bits: u8) -> Self {
        self.server_max_window_bits = server_max_window_bits.clamp(9, 15);
        self
    }

    /// The base-2 logarithm of the maximum size of the window that a client can use to compress
    /// its messages, from 9 (512 bytes) to 15 (32 KiB). A value out of this range is brought back
    /// to it. A smaller window uses less memory on the client, but its messages are less
    /// compressed. This doesn't reduce the memory used by the server to decompress them.
    ///
    /// This has no effect on a client that doesn't declare to support it in its offer.
    /// A client that asks for a smaller window than this one will use its own.
    ///
    /// Defaults to 15.
    pub fn client_max_window_bits(mut self, client_max_window_bits: u8) -> Self {
        self.client_max_window_bits = client_max_window_bits.clamp(9, 15);
        self
    }
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".into(),
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
            upgrade_timeout: Duration::from_millis(10000),
            max_buffer_size: 128,
            max_payload: 1e5 as u64, // 100kb
            ws_read_buffer_size: 4096,
            ws_max_message_size: 64 << 20,
            ws_max_frame_size: 16 << 20,
            transports: TransportType::Polling as u8 | TransportType::Websocket as u8,
            #[cfg(feature = "ws-deflate")]
            ws_per_message_deflate: None,
        }
    }
}

impl EngineIoConfig {
    /// Create a new builder with a default config
    pub fn builder() -> EngineIoConfigBuilder {
        EngineIoConfigBuilder::new()
    }

    /// Check if a [`TransportType`] is enabled in the [`EngineIoConfig`]
    #[inline(always)]
    pub fn allowed_transport(&self, transport: TransportType) -> bool {
        self.transports & transport as u8 == transport as u8
    }
}

/// Builder for [`EngineIoConfig`]
pub struct EngineIoConfigBuilder {
    config: EngineIoConfig,
}

impl EngineIoConfigBuilder {
    /// Create a new builder with a default config
    pub fn new() -> Self {
        Self {
            config: EngineIoConfig::default(),
        }
    }

    /// The path to listen for engine.io requests on.
    /// Defaults to "/engine.io".
    pub fn req_path(mut self, req_path: impl Into<Cow<'static, str>>) -> Self {
        self.config.req_path = req_path.into();
        self
    }

    /// The interval at which the server will send a ping packet to the client.
    /// Defaults to 25 seconds.
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.config.ping_interval = ping_interval;
        self
    }

    // The amount of time the server will wait for a ping response from the client before closing the connection.
    /// Defaults to 20 seconds.
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.config.ping_timeout = ping_timeout;
        self
    }

    /// The amount of time the server will wait for a transport upgrade to complete before aborting it
    /// and reclaiming the session.
    ///
    /// Defaults to 10 seconds.
    pub fn upgrade_timeout(mut self, upgrade_timeout: Duration) -> Self {
        self.config.upgrade_timeout = upgrade_timeout;
        self
    }

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    ///
    /// If the buffer if full the `emit()` method will return an error
    /// ```
    /// # use bytes::Bytes;
    /// # use engineioxide::{
    ///     Str,
    ///     layer::EngineIoLayer,
    ///     handler::EngineIoHandler,
    ///     socket::{Socket, DisconnectReason},
    /// };
    /// # use std::sync::Arc;
    /// #[derive(Debug, Clone)]
    /// struct MyHandler;
    ///
    /// impl EngineIoHandler for MyHandler {
    ///
    ///     type Data = ();
    ///     fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
    ///         println!("socket connect {}", socket.id);
    ///     }
    ///     fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) {
    ///         println!("socket disconnect {}", socket.id);
    ///     }
    ///
    ///     fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<()>>) {
    ///         println!("Ping pong message {:?}", msg);
    ///         socket.emit(msg).unwrap();
    ///     }
    ///
    ///     fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<()>>) {
    ///         println!("Ping pong binary message {:?}", data);
    ///         socket.emit_binary(data).unwrap();
    ///     }
    /// }
    /// ```
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.config.max_buffer_size = max_buffer_size;
        self
    }

    /// The maximum number of bytes that can be received per http request.
    /// Defaults to 100kb.
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.config.max_payload = max_payload;
        self
    }

    /// The size of the read buffer for the websocket transport.
    /// You can tweak this value depending on your use case. Defaults to 4KiB.
    ///
    /// Setting it to a higher value will improve performance on heavy read scenarios
    /// but will consume more memory.
    pub fn ws_read_buffer_size(mut self, ws_read_buffer_size: usize) -> Self {
        self.config.ws_read_buffer_size = ws_read_buffer_size;
        self
    }

    /// The maximum size of an incoming message
    ///
    /// The default value is 64 MiB which should be reasonably big for all normal use-cases but small enough
    /// to prevent memory eating by a malicious user.
    pub fn ws_max_message_size(mut self, ws_max_message_size: usize) -> Self {
        self.config.ws_max_message_size = ws_max_message_size;
        self
    }

    /// The maximum size of a single incoming message frame.
    /// The limit is for frame payload NOT including the frame header.
    ///
    /// The default value is 16 MiB which should be reasonably big for all normal use-cases but small enough
    /// to prevent memory eating by a malicious user.
    pub fn ws_max_frame_size(mut self, ws_max_frame_size: usize) -> Self {
        self.config.ws_max_frame_size = ws_max_frame_size;
        self
    }

    /// Allowed transports on this server
    ///
    /// The `transports` array should have a size of 1 or 2
    ///
    /// Defaults to :
    /// `[TransportType::Polling, TransportType::Websocket]`
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        assert!(N > 0 && N <= 2);
        self.config.transports = 0;
        for transport in transports {
            self.config.transports |= transport as u8;
        }
        self
    }

    /// Enable the `permessage-deflate` websocket extension for the clients that offer it.
    ///
    /// It is disabled by default. See [`PerMessageDeflateConfig`] for its costs.
    #[cfg(feature = "ws-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ws-deflate")))]
    pub fn ws_per_message_deflate(mut self, config: PerMessageDeflateConfig) -> Self {
        self.config.ws_per_message_deflate = Some(config);
        self
    }

    /// Build the config
    pub fn build(self) -> EngineIoConfig {
        self.config
    }
}
impl Default for EngineIoConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn config_transports() {
        let conf = EngineIoConfig::builder()
            .transports([TransportType::Polling])
            .build();
        assert!(conf.allowed_transport(TransportType::Polling));
        assert!(!conf.allowed_transport(TransportType::Websocket));

        let conf = EngineIoConfig::builder()
            .transports([TransportType::Websocket])
            .build();
        assert!(conf.allowed_transport(TransportType::Websocket));
        assert!(!conf.allowed_transport(TransportType::Polling));
        let conf = EngineIoConfig::builder()
            .transports([TransportType::Polling, TransportType::Websocket])
            .build();
        assert!(conf.allowed_transport(TransportType::Polling));
        assert!(conf.allowed_transport(TransportType::Websocket));
    }
}
