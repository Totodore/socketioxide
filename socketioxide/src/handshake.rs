use std::{sync::Arc, time::SystemTime};

use engineioxide::socket::SocketReq;
use serde::de::DeserializeOwned;

use crate::errors::Error;

/// Handshake informations bound to a socket
#[derive(Debug)]
pub struct Handshake {
    pub(crate) auth: serde_json::Value,
    pub issued: SystemTime,
    pub req: Arc<SocketReq>,
}

impl Handshake {
    pub(crate) fn new(auth: serde_json::Value, req: Arc<SocketReq>) -> Self {
        Self {
            auth,
            req,
            issued: SystemTime::now(),
        }
    }
    /// Extract the data from the handshake.
    ///
    /// It is cloned and deserialized from a json::Value to the given type.
    pub fn data<T: DeserializeOwned>(&self) -> Result<T, Error> {
        Ok(serde_json::from_value(self.auth.clone())?)
    }
}

#[cfg(test)]
impl Handshake {
    pub fn new_dummy() -> Self {
        Self {
            auth: serde_json::json!({}),
            issued: SystemTime::now(),
            req: Arc::new(SocketReq {
                headers: Default::default(),
                uri: Default::default(),
            }),
        }
    }
}
