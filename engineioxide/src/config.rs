use std::time::Duration;

use crate::service::TransportType;

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    /// The path to listen for engine.io requests on.
    /// Defaults to "/engine.io".
    pub req_path: String,

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
    /// Defaults to 100kb.
    pub max_payload: u64,

    /// Allowed transports on this server
    /// It is represented as a bitfield to allow to combine any number of transports easily
    pub transports: u8,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
            ping_interval: Duration::from_millis(25000),
            ping_timeout: Duration::from_millis(20000),
            max_buffer_size: 128,
            max_payload: 1e5 as u64, // 100kb
            transports: TransportType::Polling as u8 | TransportType::Websocket as u8,
        }
    }
}

impl EngineIoConfig {
    pub fn builder() -> EngineIoConfigBuilder {
        EngineIoConfigBuilder::new()
    }

    /// Check if a [`TransportType`] is enabled in the [`EngineIoConfig`]
    #[inline(always)]
    pub fn allowed_transport(&self, transport: TransportType) -> bool {
        self.transports & transport as u8 == transport as u8
    }
}
pub struct EngineIoConfigBuilder {
    config: EngineIoConfig,
}

impl EngineIoConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: EngineIoConfig::default(),
        }
    }

    /// The path to listen for engine.io requests on.
    /// Defaults to "/engine.io".
    pub fn req_path(mut self, req_path: String) -> Self {
        self.config.req_path = req_path;
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
    /// # use engineioxide::{
    ///     layer::EngineIoLayer,
    ///     handler::EngineIoHandler,
    ///     socket::Socket,
    /// };
    /// # use std::sync::Arc;
    /// #[derive(Debug, Clone)]
    /// struct MyHandler;
    ///
    /// #[engineioxide::async_trait]
    /// impl EngineIoHandler for MyHandler {
    ///
    ///     type Data = ();
    ///     fn on_connect(&self, socket: &Socket<Self>) {
    ///         println!("socket connect {}", socket.sid);
    ///     }
    ///     fn on_disconnect(&self, socket: &Socket<Self>) {
    ///         println!("socket disconnect {}", socket.sid);
    ///     }
    ///
    ///     fn on_message(&self, msg: String, socket: &Socket<Self>) {
    ///         println!("Ping pong message {:?}", msg);
    ///         socket.emit(msg).unwrap();
    ///     }
    ///
    ///     fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) {
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
    use crate::service::TransportType;

    use super::EngineIoConfig;

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
