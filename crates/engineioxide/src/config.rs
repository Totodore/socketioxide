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

    /// The maximum size of an incoming websocket message.
    ///
    /// Defaults to 100KB, like [`max_payload`](Self::max_payload).
    /// The [`EngineIoConfigBuilder`] sets it to the configured `max_payload` unless
    /// [`EngineIoConfigBuilder::ws_max_message_size`] is called.
    /// If you build this struct by hand, you should keep the two values in sync.
    pub ws_max_message_size: usize,

    /// The maximum size of a single incoming websocket frame.
    /// The limit is for frame payload NOT including the frame header.
    ///
    /// A frame is fully buffered before its size is added to the size of the current message,
    /// so this limit should not be greater than [`ws_max_message_size`](Self::ws_max_message_size).
    ///
    /// Defaults to 100KB. The [`EngineIoConfigBuilder`] sets it to the configured
    /// `ws_max_message_size` unless [`EngineIoConfigBuilder::ws_max_frame_size`] is called.
    pub ws_max_frame_size: usize,

    /// Allowed transports on this server
    /// It is represented as a bitfield to allow to combine any number of transports easily
    pub transports: u8,
}

/// 100KB
const DEFAULT_MAX_PAYLOAD: u64 = 100_000;

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".into(),
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
            upgrade_timeout: Duration::from_millis(10000),
            max_buffer_size: 128,
            max_payload: DEFAULT_MAX_PAYLOAD,
            ws_read_buffer_size: 4096,
            ws_max_message_size: DEFAULT_MAX_PAYLOAD as usize,
            ws_max_frame_size: DEFAULT_MAX_PAYLOAD as usize,
            transports: TransportType::Polling as u8 | TransportType::Websocket as u8,
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
    ws_max_message_size_set: bool,
    ws_max_frame_size_set: bool,
}

impl EngineIoConfigBuilder {
    /// Create a new builder with a default config
    pub fn new() -> Self {
        Self {
            config: EngineIoConfig::default(),
            ws_max_message_size_set: false,
            ws_max_frame_size_set: false,
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
    ///
    /// It also applies to incoming websocket messages unless
    /// [`ws_max_message_size`](Self::ws_max_message_size) is set.
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

    /// The maximum size of an incoming websocket message.
    ///
    /// Defaults to [`max_payload`](Self::max_payload). Setting a higher value allows a client
    /// to make the server buffer and parse this amount of data per message.
    pub fn ws_max_message_size(mut self, ws_max_message_size: usize) -> Self {
        self.config.ws_max_message_size = ws_max_message_size;
        self.ws_max_message_size_set = true;
        self
    }

    /// The maximum size of a single incoming websocket frame.
    /// The limit is for frame payload NOT including the frame header.
    ///
    /// Defaults to [`ws_max_message_size`](Self::ws_max_message_size). A frame is fully buffered
    /// before its size is added to the size of the current message, so this limit should not be
    /// greater than `ws_max_message_size`.
    pub fn ws_max_frame_size(mut self, ws_max_frame_size: usize) -> Self {
        self.config.ws_max_frame_size = ws_max_frame_size;
        self.ws_max_frame_size_set = true;
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

    /// Build the config
    pub fn build(mut self) -> EngineIoConfig {
        if !self.ws_max_message_size_set {
            self.config.ws_max_message_size =
                usize::try_from(self.config.max_payload).unwrap_or(usize::MAX);
        }
        if !self.ws_max_frame_size_set {
            self.config.ws_max_frame_size = self.config.ws_max_message_size;
        }
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

    #[test]
    fn default_ws_limits_match_max_payload() {
        let config = EngineIoConfig::default();
        assert_eq!(config.ws_max_message_size as u64, config.max_payload);
        assert_eq!(config.ws_max_frame_size as u64, config.max_payload);

        let config = EngineIoConfig::builder().build();
        assert_eq!(config.ws_max_message_size as u64, config.max_payload);
        assert_eq!(config.ws_max_frame_size as u64, config.max_payload);
    }

    #[test]
    fn ws_limits_follow_max_payload() {
        let config = EngineIoConfig::builder().max_payload(1_000_000).build();
        assert_eq!(config.ws_max_message_size, 1_000_000);
        assert_eq!(config.ws_max_frame_size, 1_000_000);
    }

    #[test]
    fn ws_frame_limit_follows_ws_message_limit() {
        let config = EngineIoConfig::builder()
            .ws_max_message_size(5_000)
            .max_payload(1_000)
            .build();
        assert_eq!(config.max_payload, 1_000);
        assert_eq!(config.ws_max_message_size, 5_000);
        assert_eq!(config.ws_max_frame_size, 5_000);
    }

    #[test]
    fn explicit_ws_frame_limit_is_kept() {
        let config = EngineIoConfig::builder()
            .ws_max_frame_size(2_000)
            .max_payload(1_000)
            .build();
        assert_eq!(config.ws_max_message_size, 1_000);
        assert_eq!(config.ws_max_frame_size, 2_000);
    }
}
