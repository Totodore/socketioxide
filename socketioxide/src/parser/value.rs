//! Value enum with all possible implementations for each parser
use bytes::Bytes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Json(serde_json::Value),
    MsgPack(MsgPackValue),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MsgPackValue {
    pub(crate) data: Bytes,
    pub(crate) attachments: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("json error: {0}")]
    Json(#[from] serde_json::error::Error),
    #[error("msgpack encode error: {0}")]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode error: {0}")]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
}

impl Value {
    pub(crate) fn to_msgpack(self) -> Option<MsgPackValue> {
        match self {
            Value::Json(_) => None,
            Value::MsgPack(data) => Some(data),
        }
    }
    pub(crate) fn to_json(self) -> Option<serde_json::Value> {
        match self {
            Value::Json(data) => Some(data),
            Value::MsgPack(_) => None,
        }
    }
    pub(crate) fn has_binary(&self) -> bool {
        match &self {
            Value::MsgPack(MsgPackValue { attachments, .. }) => *attachments > 0,
            Value::Json(_) => false,
        }
    }
}
impl<T: Into<serde_json::Value>> From<T> for Value {
    fn from(value: T) -> Self {
        Value::Json(value.into())
    }
}
impl From<MsgPackValue> for Value {
    fn from(value: MsgPackValue) -> Self {
        Value::MsgPack(value)
    }
}

/// Converts a instance of [`Value`] to a given type [`T`]
pub fn from_value<'de, T: DeserializeOwned>(value: &Value) -> Result<T, ParseError> {
    let res = match value {
        Value::Json(v) => T::deserialize(v)?,
        Value::MsgPack(v) => rmp_serde::decode::from_slice(&v.data)?,
    };
    Ok(res)
}
