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

    /// Allowed transports on this server
    /// It is represented as a bitfield to allow to combine any number of transports easily
    pub transports: u8,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".into(),
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
            max_buffer_size: 128,
            max_payload: 1e5 as u64, // 100kb
            ws_read_buffer_size: 4096,
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
