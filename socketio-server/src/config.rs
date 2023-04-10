use std::time::Duration;

use engineio_server::layer::EngineIoConfig;

pub struct SocketIoConfigBuilder {
    config: SocketIoConfig,
}

impl SocketIoConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: SocketIoConfig::default(),
        }
    }
    pub fn req_path(mut self, req_path: String) -> Self {
        self.config.engine_config.req_path = req_path;
        self
    }
    pub fn ping_interval(mut self, ping_interval: Duration) -> Self {
        self.config.engine_config.ping_interval = ping_interval;
        self
    }
    pub fn ping_timeout(mut self, ping_timeout: Duration) -> Self {
        self.config.engine_config.ping_timeout = ping_timeout;
        self
    }
    pub fn max_buffer_size(mut self, max_buffer_size: usize) -> Self {
        self.config.engine_config.max_buffer_size = max_buffer_size;
        self
    }
    pub fn max_payload(mut self, max_payload: u64) -> Self {
        self.config.engine_config.max_payload = max_payload;
        self
    }

    pub fn ack_timeout(mut self, ack_timeout: Duration) -> Self {
        self.config.ack_timeout = ack_timeout;
        self
    }
    pub fn build(self) -> SocketIoConfig {
        self.config
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
