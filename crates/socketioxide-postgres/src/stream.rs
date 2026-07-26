use serde::de::DeserializeOwned;

/// A resolved ack-response payload carrying either a JSON `RawValue` (inline data or a
/// JSON-encoded attachment) or raw msgpack bytes (a binary attachment). The format
/// has to travel with the payload because the eventual `Response<E>` is decoded
/// downstream, where the right serde codec is selected.
pub enum AckPayload {
    Json(Box<serde_json::value::RawValue>),
    MsgPack(Vec<u8>),
}

impl AckPayload {
    pub(crate) fn deserialize<T: DeserializeOwned>(
        &self,
    ) -> Result<T, Box<dyn std::error::Error + Send>> {
        match &self {
            AckPayload::Json(rv) => serde_json::from_str::<T>(rv.get())
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>),
            AckPayload::MsgPack(bytes) => rmp_serde::from_slice::<T>(bytes)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>),
        }
    }
}
