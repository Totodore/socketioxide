use std::str::FromStr;

use engineioxide_core::TransportType;
use http::Uri;

use crate::errors::ConfigError;

/// Configuration for the Engine.io client.
#[derive(Debug)]
pub struct EngineIoClientConfig {
    /// A list of transports to try (in order). Engine.io always attempts to
    /// connect directly with the first one, provided the feature detection test
    /// for it passes.
    ///
    /// Defaults to `[Polling, Websocket]`.
    ///
    /// <div class="warning">
    ///     With only <code>flavor-hyper</code> enabled, only <code>Polling</code> is supported.
    /// </div>
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
    /// Returns a builder for constructing an [`EngineIoClientConfig`].
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

pub trait IntoEngineIoClientConfig {
    fn into_config(self) -> Result<EngineIoClientConfig, ConfigError>;
}
impl IntoEngineIoClientConfig for EngineIoClientConfig {
    fn into_config(self) -> Result<EngineIoClientConfig, ConfigError> {
        Ok(self)
    }
}
impl IntoEngineIoClientConfig for &str {
    fn into_config(self) -> Result<EngineIoClientConfig, ConfigError> {
        EngineIoClientConfigBuilder::new().uri(self).build()
    }
}
impl IntoEngineIoClientConfig for Result<EngineIoClientConfig, ConfigError> {
    fn into_config(self) -> Result<EngineIoClientConfig, ConfigError> {
        self
    }
}
impl<const N: usize> IntoEngineIoClientConfig for [TransportType; N] {
    fn into_config(self) -> Result<EngineIoClientConfig, ConfigError> {
        EngineIoClientConfigBuilder::new().transports(self).build()
    }
}
impl FromStr for EngineIoClientConfig {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EngineIoClientConfigBuilder::new().uri(s).build()
    }
}

/// Builder for constructing an [`EngineIoClientConfig`].
#[derive(Default)]
pub struct EngineIoClientConfigBuilder {
    config: EngineIoClientConfig,
    uri: Option<String>,
}
impl EngineIoClientConfigBuilder {
    /// Returns a new [`EngineIoClientConfigBuilder`] with default values.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets the URI for the engine.io client.
    pub fn uri(mut self, uri: &str) -> Self {
        self.uri = Some(uri.to_string());
        self
    }

    /// Sets available transports for the engine.io client.
    ///
    /// <div class="warning">
    ///     With only <code>flavor-hyper</code> enabled, only <code>Polling</code> is supported.
    /// </div>
    pub fn transports<const N: usize>(mut self, transports: [TransportType; N]) -> Self {
        const { assert!(N > 0, "transports list should be non-empty") };

        self.config.transports = transports.to_vec();
        self
    }

    /// Builds the [`EngineIoClientConfig`] from the builder's state.
    pub fn build(mut self) -> Result<EngineIoClientConfig, ConfigError> {
        if let Some(uri) = self.uri {
            self.config.uri = uri.parse()?;
        }
        Ok(self.config)
    }
}
