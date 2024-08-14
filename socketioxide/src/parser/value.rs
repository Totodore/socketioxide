//! Value enum with all possible implementations for each parser
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Value {
    MsgPack(rmpv::Value),
    Json(serde_json::Value),
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("msgpack error: {0}")]
    MsgPack(#[from] rmpv::ext::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::error::Error),
}
impl From<rmpv::Value> for Value {
    fn from(value: rmpv::Value) -> Self {
        Value::MsgPack(value)
    }
}

impl From<serde_json::Value> for Value {
    fn from(value: serde_json::Value) -> Self {
        Value::Json(value)
    }
}

pub fn from_value<'de, T: DeserializeOwned>(value: Value) -> Result<T, ParseError> {
    let res = match value {
        Value::Json(v) => serde_json::from_value(v)?,
        Value::MsgPack(v) => rmpv::ext::from_value(v)?,
    };
    Ok(res)
}
