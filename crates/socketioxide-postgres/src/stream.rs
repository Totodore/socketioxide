use std::{
    fmt,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use futures_core::{FusedStream, Stream};
use futures_util::{StreamExt, stream::TakeUntil};
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;
use socketioxide_core::{
    adapter::AckStreamItem,
    adapter::remote_packet::{Response, ResponseType},
};
use tokio::{sync::mpsc, time};

use crate::drivers::{NotifStream, Notification};

pin_project! {
    /// A stream of acknowledgement messages received from the local and remote servers.
    /// It merges the local ack stream with the remote ack stream from all the servers.
    // The server_cnt is the number of servers that are expected to send a AckCount message.
    // It is decremented each time a AckCount message is received.
    //
    // The ack_cnt is the number of acks that are expected to be received. It is the sum of all the the ack counts.
    // And it is decremented each time an ack is received.
    //
    // Therefore an exhausted stream correspond to `ack_cnt == 0` and `server_cnt == 0`.
    pub struct AckStream<S, T: Notification> {
        #[pin]
        local: S,
        #[pin]
        remote: TakeUntil<NotifStream<T>, time::Sleep>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<S, T: Notification> AckStream<S, T> {
    pub fn new(local: S, remote: NotifStream<T>, timeout: Duration, serv_cnt: u16) -> Self {
        let remote = remote.take_until(time::sleep(timeout));
        Self {
            local,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt,
            remote,
        }
    }

    pub fn new_local(local: S) -> Self {
        let rx = mpsc::unbounded_channel().1;
        let remote = NotifStream::new(rx).take_until(time::sleep(Duration::ZERO));
        Self {
            local,
            remote,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt: 0,
        }
    }
}
impl<Err, S, T: Notification> AckStream<S, T>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
{
    /// Poll the remote stream. First the count of acks is received, then the acks are received.
    /// We expect `serv_cnt` of `BroadcastAckCount` messages to be received, then we expect
    /// `ack_cnt` of `BroadcastAck` messages.
    fn poll_remote<E: DeserializeOwned + fmt::Debug>(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<E>>> {
        // remote stream is not fused, so we need to check if it is terminated
        if FusedStream::is_terminated(&self) {
            return Poll::Ready(None);
        }
        let mut projection = self.project();
        loop {
            match projection.remote.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(notif)) => {
                    let channel = notif.channel();
                    let res = serde_json::from_str::<Response<E>>(notif.payload());
                    match res {
                        Ok(Response {
                            node_id: uid,
                            r#type: ResponseType::BroadcastAckCount(count),
                        }) if *projection.serv_cnt > 0 => {
                            tracing::trace!(?uid, channel, "receiving broadcast ack count {count}");
                            *projection.ack_cnt += count;
                            *projection.total_ack_cnt += count as usize;
                            *projection.serv_cnt -= 1;
                        }
                        Ok(Response {
                            node_id: uid,
                            r#type: ResponseType::BroadcastAck((sid, res)),
                        }) if *projection.ack_cnt > 0 => {
                            tracing::trace!(
                                ?uid,
                                channel,
                                "receiving broadcast ack {sid} {:?}",
                                res
                            );
                            *projection.ack_cnt -= 1;
                            return Poll::Ready(Some((sid, res)));
                        }
                        Ok(Response { node_id: uid, .. }) => {
                            tracing::warn!(?uid, channel, "unexpected response type");
                        }
                        Err(e) => {
                            tracing::warn!("error decoding ack response: {e}");
                        }
                    }
                }
            }
        }
    }
}
impl<E, S, T> Stream for AckStream<S, T>
where
    E: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<E>> + FusedStream,
    T: Notification,
{
    type Item = AckStreamItem<E>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project().local.poll_next(cx) {
            Poll::Pending => match self.poll_remote(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
                Poll::Ready(None) => Poll::Pending,
            },
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => self.poll_remote(cx),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, upper) = self.local.size_hint();
        (lower, upper.map(|upper| upper + self.total_ack_cnt))
    }
}

impl<Err, S, T> FusedStream for AckStream<S, T>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
    T: Notification,
{
    /// The stream is terminated if:
    /// * The local stream is terminated.
    /// * All the servers have sent the expected ack count.
    /// * We have received all the expected acks.
    fn is_terminated(&self) -> bool {
        // remote stream is terminated if the timeout is reached
        let remote_term = (self.ack_cnt == 0 && self.serv_cnt == 0) || self.remote.is_terminated();
        self.local.is_terminated() && remote_term
    }
}
impl<S, T: Notification> fmt::Debug for AckStream<S, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AckStream")
            .field("ack_cnt", &self.ack_cnt)
            .field("total_ack_cnt", &self.total_ack_cnt)
            .field("serv_cnt", &self.serv_cnt)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use futures_core::FusedStream;
    use futures_util::StreamExt;
    use socketioxide_core::{Sid, Value};

    use super::AckStream;

    #[tokio::test]
    async fn local_ack_stream_should_have_a_closed_remote() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, ()>(Value::Str("local".into(), None)))
        });
        let stream = AckStream::new_local(local);
        futures_util::pin_mut!(stream);
        assert_eq!(stream.ack_cnt, 0);
        assert_eq!(stream.total_ack_cnt, 0);
        assert_eq!(stream.serv_cnt, 0);
        assert!(!stream.local.is_terminated());
        assert!(!stream.is_terminated());
        let data = stream.next().await;
        assert!(
            matches!(data, Some((id, Ok(Value::Str(msg, None)))) if id == sid && msg == "local")
        );
        assert_eq!(stream.next().await, None);
        assert!(stream.is_terminated());
    }
}
