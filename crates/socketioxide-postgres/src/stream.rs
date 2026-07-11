use std::fmt;

use serde::Deserialize;
use serde_json::value::RawValue;
use socketioxide_core::adapter::remote_packet::Response;

/// A resolved ack-response payload carrying either a JSON `RawValue` (inline data or a
/// JSON-encoded attachment) or raw msgpack bytes (a binary attachment). The format
/// has to travel with the payload because the eventual `Response<E>` is decoded
/// downstream, where the right serde codec is selected.
pub enum AckPayload {
    Json(Box<RawValue>),
    MsgPack(Vec<u8>),
}

impl AckPayload {
    fn deserialize<'de, T: Deserialize<'de>>(&'de self) -> Result<T, Box<dyn std::error::Error + Send>> {
        match &self {
            AckPayload::Json(rv) => serde_json::from_str::<T>(rv.get())
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>),
            AckPayload::MsgPack(bytes) => rmp_serde::from_slice::<T>(bytes)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>),
        }
    }
}

/// Decode an `AckPayload` into a `Response<E>` using either JSON or msgpack.
pub fn decode_postgres_ack<E: serde::de::DeserializeOwned + fmt::Debug>(
    item: AckPayload,
) -> Result<Response<E>, Box<dyn std::error::Error + Send>> {
    item.deserialize()
}

#[cfg(test)]
mod tests {
    use futures_core::FusedStream;
    use futures_util::StreamExt;
    use socketioxide_core::{Sid, Value};

    use crate::stream::{AckPayload, decode_postgres_ack};
    use socketioxide_core::adapter::stream::AckStream;

    #[tokio::test]
    async fn local_ack_stream_should_have_a_closed_remote() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, ()>(Value::Str("local".into(), None)))
        });
        let empty: futures_core::stream::BoxStream<'static, AckPayload> =
            futures_util::stream::empty().boxed();
        let stream: AckStream<_, _, crate::ResponsePayload, ()> = AckStream::new_empty_remote(local, empty, decode_postgres_ack::<()>);
        futures_util::pin_mut!(stream);
        assert!(!FusedStream::is_terminated(&stream));
        let data = stream.next().await;
        assert!(
            matches!(data, Some((id, Ok(Value::Str(msg, None)))) if id == sid && msg == "local")
        );
        assert_eq!(stream.next().await, None);
        assert!(FusedStream::is_terminated(&stream));
    }
}
