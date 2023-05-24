use serde::de::DeserializeOwned;

use crate::errors::Error;

//TODO: add http headerMap
/// Handshake informations bound to a socket
#[derive(Debug)]
pub struct Handshake {
    pub(crate) auth: serde_json::Value,
    pub url: String,
    // pub headers: HeaderMap<HeaderValue>,
    pub issued: u64,
}

impl Handshake {
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
            url: "http://localhost".to_string(),
            // headers: HeaderMap::new(),
            issued: 0,
        }
    }
}