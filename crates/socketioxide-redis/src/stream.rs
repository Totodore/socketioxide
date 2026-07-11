use std::fmt;

use serde::de::DeserializeOwned;
use socketioxide_core::{
    Sid,
    adapter::remote_packet::Response,
};

/// Decode a msgpack-encoded `(Sid, Response<E>)` tuple from raw bytes.
pub fn decode_redis_ack<E: DeserializeOwned + fmt::Debug>(
    item: Vec<u8>,
) -> Result<Response<E>, Box<dyn std::error::Error + Send>> {
    rmp_serde::from_slice::<(Sid, Response<E>)>(&item)
        .map(|(_, response)| response)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)
}

#[cfg(test)]
mod tests {
    use futures_core::FusedStream;
    use futures_util::StreamExt;
    use socketioxide_core::{Sid, Value};

    use crate::drivers::MessageStream;
    use crate::stream::decode_redis_ack;
    use socketioxide_core::adapter::stream::AckStream;

    #[tokio::test]
    async fn local_ack_stream_should_have_a_closed_remote() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, ()>(Value::Str("local".into(), None)))
        });
        let stream: AckStream<_, _, Vec<u8>, ()> = AckStream::new_empty_remote(local, MessageStream::<Vec<u8>>::new_empty(), decode_redis_ack::<()>);
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
