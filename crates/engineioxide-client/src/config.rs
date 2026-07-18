use engineioxide_core::TransportType;
use http::Uri;

#[derive(Debug)]
pub struct EngineIoClientConfig {
    /// A list of transports to try (in order). Engine.io always attempts to
    /// connect directly with the first one, provided the feature detection test
    /// for it passes.
    ///
    /// Defaults to `[Polling, Websocket]`.
    pub transports: Vec<TransportType>,

    /// The uri to use to connect to the server.
    ///
    /// Defaults to `http://localhost/engine.io`.
    pub uri: Uri,
}

impl Default for EngineIoClientConfig {
    fn default() -> Self {
        Self {
            uri: Uri::from_static("http://localhost/engine.io"),
            transports: vec![TransportType::Polling, TransportType::Websocket],
        }
    }
}

impl EngineIoClientConfig {
    pub fn builder() -> EngineIoClientConfigBuilder {
        EngineIoClientConfigBuilder::new()
    }

    pub(crate) fn initial_transport(&self) -> TransportType {
        *self
            .transports
            .first()
            .expect("transport list should never be empty")
    }
}

#[derive(Default)]
pub struct EngineIoClientConfigBuilder {
    config: EngineIoClientConfig,
    uri: Option<String>,
}
impl EngineIoClientConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn uri(mut self, uri: &str) -> Self {
        self.uri = Some(uri.to_string());
        self
    }
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        const { assert!(N > 0, "transports list should be non-empty") };

        self.config.transports = transports.to_vec();
        self
    }
    pub fn build(mut self) -> EngineIoClientConfig {
        if let Some(uri) = self.uri {
            self.config.uri = uri.parse().unwrap(); //TODO: err
        }
        self.config
    }
}
