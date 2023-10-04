use std::time::Duration;

use engineioxide::config::EngineIoConfig;
pub use engineioxide::service::TransportType;

/// Configuration for Socket.IO & Engine.IO
#[derive(Debug, Clone)]
pub struct SocketIoConfig {
    /// The inner Engine.IO config
    pub engine_config: EngineIoConfig,

    /// The amount of time the server will wait for an acknowledgement from the client before closing the connection.
    ///
    /// Defaults to 5 seconds.
    pub ack_timeout: Duration,

    /// The amount of time before disconnecting a client that has not successfully joined a namespace.
    ///
    /// Defaults to 45 seconds.
    pub connect_timeout: Duration,
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
