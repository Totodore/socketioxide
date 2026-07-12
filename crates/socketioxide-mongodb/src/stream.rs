use std::fmt;

use serde::de::DeserializeOwned;
use socketioxide_core::adapter::remote_packet::Response;

use crate::drivers::Item;

/// Decode a MongoDB `Item`'s msgpack data into a `Response<E>`.
pub fn decode_mongodb_ack<E: DeserializeOwned + fmt::Debug>(
    item: Item,
) -> Result<Response<E>, Box<dyn std::error::Error + Send>> {
    rmp_serde::from_slice::<Response<E>>(&item.data)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)
}

#[cfg(test)]
mod tests {
    use futures_core::FusedStream;
    use futures_util::StreamExt;
    use socketioxide_core::{Sid, Value};
    use tokio::sync::mpsc;

    use crate::stream::decode_mongodb_ack;
    use socketioxide_core::adapter::stream::{AckStream, ChanStream};

    #[tokio::test]
    async fn local_ack_stream_should_have_a_closed_remote() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, ()>(Value::Str("local".into(), None)))
        });
        let (_, rx) = mpsc::channel::<crate::drivers::Item>(1);
        let stream: AckStream<_, _, crate::drivers::Item, ()> =
            AckStream::new_empty_remote(local, ChanStream::new(rx), decode_mongodb_ack::<()>);
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
