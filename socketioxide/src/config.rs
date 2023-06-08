use std::time::Duration;

use engineioxide::config::EngineIoConfig;

/// Builder for SocketIoConfig
pub struct SocketIoConfigBuilder {
    config: SocketIoConfig,
}

impl SocketIoConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
        }
    }

    /// The path to listen for socket.io requests on.
    /// Defaults to "/socket.io".
    pub fn req_path(mut self, req_path: String) -> Self {
        self.config.engine_config.req_path = req_path;
        self
    }

    /// The interval at which the server will send a ping packet to the client.
    /// Defaults to 25 seconds.
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.config.engine_config.ping_interval = ping_interval;
        self
    }

    // The amount of time the server will wait for a ping response from the client before closing the connection.
    /// Defaults to 20 seconds.
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.config.engine_config.ping_timeout = ping_timeout;
        self
    }

    /// The maximum number of packets that can be buffered per connection before being emitted to the client.
    /// If the buffer if full the `emit()` method will return an error
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.config.engine_config.max_buffer_size = max_buffer_size;
        self
    }

    /// The maximum size of a payload in bytes.
    /// If a payload is bigger than this value the `emit()` method will return an error
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.config.engine_config.max_payload = max_payload;
        self
    }

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    /// Defaults to 5 seconds.
    pub fn ack_timeout(mut self, ack_timeout: Duration) -> Self {
        self.config.ack_timeout = ack_timeout;
        self
    }

    /// Build the config
    pub fn build(self) -> SocketIoConfig {
        self.config
    }
}

impl Default for SocketIoConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SocketIoConfig {
    pub(crate) engine_config: EngineIoConfig,
    pub(crate) ack_timeout: Duration,
}

impl Default for SocketIoConfig {
    fn default() -> Self {
        Self {
            engine_config: EngineIoConfig {
                req_path: "/socket.io".to_string(),
                ..Default::default()
            },
            ack_timeout: Duration::from_secs(5),
        }
    }
}
impl SocketIoConfig {
    pub fn builder() -> SocketIoConfigBuilder {
        SocketIoConfigBuilder::new()
    }
}
