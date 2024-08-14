use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    MsgPack(rmpv::Value),
    Json(serde_json::Value),
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
