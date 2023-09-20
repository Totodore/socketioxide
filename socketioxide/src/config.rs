use std::time::Duration;

use engineioxide::config::{EngineIoConfig, EngineIoConfigBuilder};
pub use engineioxide::service::TransportType;

/// Builder for SocketIoConfig
pub struct SocketIoConfigBuilder {
    config: SocketIoConfig,

    engine_config_builder: EngineIoConfigBuilder,

    /// `req_path` default is different between engine.io and socket.io so it needs to be in a separate prop
    req_path: String,
}

impl SocketIoConfigBuilder {
    /// Create a new config builder
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
            engine_config_builder: EngineIoConfig::builder(),

            req_path: "/socket.io".to_string(),
        }
    }

    /// The path to listen for socket.io requests on.
    ///
    /// Defaults to "/socket.io".
    pub fn req_path(mut self, req_path: String) -> Self {
        self.req_path = req_path;
        self
    }

    /// The interval at which the server will send a ping packet to the client.
    ///
    /// Defaults to 25 seconds.
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_interval(ping_interval);
        self
    }

    /// The amount of time the server will wait for a ping response from the client before closing the connection.
    ///
    /// Defaults to 20 seconds.
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.engine_config_builder = self.engine_config_builder.ping_timeout(ping_timeout);
        self
    }

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    /// If the buffer if full the `emit()` method will return an error
    ///
    /// Defaults to 128 packets.
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_buffer_size(max_buffer_size);
        self
    }

    /// The maximum size of a payload in bytes.
    /// If a payload is bigger than this value the `emit()` method will return an error.
    ///
    /// Defaults to 100 kb.
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.engine_config_builder = self.engine_config_builder.max_payload(max_payload);
        self
    }

    /// Allowed transports on this server
    ///
    /// The `transports` array should have a size of 1 or 2
    ///
    /// Defaults to :
    /// `[TransportType::Polling, TransportType::Websocket]`
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        self.engine_config_builder = self.engine_config_builder.transports(transports);
        self
    }

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    pub fn ack_timeout(mut self, ack_timeout: Duration) -> Self {
        self.config.ack_timeout = ack_timeout;
        self
    }

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.config.connect_timeout = connect_timeout;
        self
    }

    /// Build the config
    pub fn build(mut self) -> SocketIoConfig {
        // `req_path` default is different between engine.io && socket.io
        self.config.engine_config = self.engine_config_builder.req_path(self.req_path).build();
        self.config
    }
}

impl Default for SocketIoConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for Socket.IO & Engine.IO
#[derive(Debug, Clone)]
pub struct SocketIoConfig {
    /// The inner Engine.IO config
    pub(crate) engine_config: EngineIoConfig,

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    pub(crate) ack_timeout: Duration,

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    pub(crate) connect_timeout: Duration,
}

impl Default for SocketIoConfig {
    fn default() -> Self {
        Self {
            engine_config: EngineIoConfig {
                req_path: "/socket.io".to_string(),
                ..Default::default()
            },
            ack_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(45),
        }
    }
}
impl SocketIoConfig {
    pub fn builder() -> SocketIoConfigBuilder {
        SocketIoConfigBuilder::new()
    }
}
