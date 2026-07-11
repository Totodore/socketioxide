use std::{
    collections::HashMap,
    fmt,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{self, Poll},
    time::Duration,
};

use futures_core::{FusedStream, Stream};
use futures_util::{StreamExt, stream::TakeUntil};
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;
use tokio::{sync::mpsc, time};

use crate::Sid;
use crate::adapter::AckStreamItem;
use crate::adapter::remote_packet::{Response, ResponseType};

/// A map of request IDs to response senders, used to route responses to the correct awaiting stream.
pub type ResponseHandlers<T> = HashMap<Sid, mpsc::Sender<T>>;

pin_project! {
    /// A [`Stream`] that wraps an [`mpsc::Receiver`].
    pub struct ChanStream<T> {
        #[pin]
        rx: mpsc::Receiver<T>,
    }
}
impl<T> ChanStream<T> {
    /// Create a new `ChanStream` from an [`mpsc::Receiver`].
    pub fn new(rx: mpsc::Receiver<T>) -> Self {
        Self { rx }
    }
}
impl<T> Stream for ChanStream<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx)
    }
}

pin_project! {
    /// A stream that unsubscribes from its source channel when dropped.
    pub struct DropStream<S, T> {
        #[pin]
        stream: S,
        req_id: Sid,
        handlers: Arc<Mutex<ResponseHandlers<T>>>,
    }
    impl<S, T> PinnedDrop for DropStream<S, T> {
        fn drop(this: Pin<&mut Self>) {
            let stream = this.project();
            let chan = stream.req_id;
            tracing::debug!(?chan, "dropping stream");
            stream.handlers.lock().unwrap().remove(chan);
        }
    }
}
impl<S, T> DropStream<S, T> {
    /// Create a new `DropStream` that will remove the handler entry on drop.
    pub fn new(stream: S, handlers: Arc<Mutex<ResponseHandlers<T>>>, req_id: Sid) -> Self {
        Self {
            stream,
            handlers,
            req_id,
        }
    }
}
impl<S: Stream, T> Stream for DropStream<S, T> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}
impl<S: FusedStream, T> FusedStream for DropStream<S, T> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

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
    pub struct AckStream<S, R: Stream, T, E> {
        #[pin]
        local: S,
        #[pin]
        remote: DropStream<TakeUntil<R, time::Sleep>, T>,
        decode: fn(R::Item) -> Result<Response<E>, Box<dyn std::error::Error + Send>>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<S, R: Stream, T, E> AckStream<S, R, T, E> {
    /// Create a new `AckStream` wrapping the given local and remote streams.
    pub fn new(
        local: S,
        remote: R,
        decode: fn(R::Item) -> Result<Response<E>, Box<dyn std::error::Error + Send>>,
        timeout: Duration,
        serv_cnt: u16,
        req_id: Sid,
        handlers: Arc<Mutex<ResponseHandlers<T>>>,
    ) -> Self {
        let remote = remote.take_until(time::sleep(timeout));
        let remote = DropStream::new(remote, handlers, req_id);
        Self {
            local,
            remote,
            decode,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt,
        }
    }

    /// Create a new `AckStream` for local-only use, with an empty remote stream.
    pub fn new_empty_remote(local: S, empty_remote: R, decode: fn(R::Item) -> Result<Response<E>, Box<dyn std::error::Error + Send>>) -> Self {
        let handlers = Arc::new(Mutex::new(ResponseHandlers::<T>::new()));
        let remote = empty_remote.take_until(time::sleep(Duration::ZERO));
        let remote = DropStream::new(remote, handlers, Sid::ZERO);
        Self {
            local,
            remote,
            decode,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt: 0,
        }
    }
}

impl<Err, S, R: Stream, T> AckStream<S, R, T, Err>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
{
    /// Poll the remote stream. First the count of acks is received, then the acks are received.
    /// We expect `serv_cnt` of `BroadcastAckCount` messages to be received, then we expect
    /// `ack_cnt` of `BroadcastAck` messages.
    fn poll_remote(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<Err>>> {
        if FusedStream::is_terminated(&self) {
            return Poll::Ready(None);
        }
        let mut projection = self.project();
        loop {
            match projection.remote.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(item)) => match (projection.decode)(item) {
                    Ok(Response {
                        node_id: uid,
                        r#type: ResponseType::BroadcastAckCount(count),
                    }) if *projection.serv_cnt > 0 => {
                        tracing::trace!(?uid, "receiving broadcast ack count {count}");
                        *projection.ack_cnt += count;
                        *projection.total_ack_cnt += count as usize;
                        *projection.serv_cnt -= 1;
                    }
                    Ok(Response {
                        node_id: uid,
                        r#type: ResponseType::BroadcastAck((sid, res)),
                    }) if *projection.ack_cnt > 0 => {
                        tracing::trace!(?uid, "receiving broadcast ack {sid} {:?}", res);
                        *projection.ack_cnt -= 1;
                        return Poll::Ready(Some((sid, res)));
                    }
                    Ok(Response { node_id: uid, .. }) => {
                        tracing::warn!(?uid, "unexpected response type");
                    }
                    Err(e) => {
                        tracing::warn!("error decoding ack response: {e}");
                    }
                },
            }
        }
    }
}

impl<Err, S, R: Stream, T> Stream for AckStream<S, R, T, Err>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
{
    type Item = AckStreamItem<Err>;

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

impl<Err, S, R: Stream, T> FusedStream for AckStream<S, R, T, Err>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
{
    /// The stream is terminated if:
    /// * The local stream is terminated.
    /// * All the servers have sent the expected ack count.
    /// * We have received all the expected acks.
    fn is_terminated(&self) -> bool {
        let remote_term = (self.ack_cnt == 0 && self.serv_cnt == 0) || self.remote.is_terminated();
        self.local.is_terminated() && remote_term
    }
}

impl<S, R: Stream, T, E> fmt::Debug for AckStream<S, R, T, E> {
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
    use crate::{Sid, Value};

    use super::AckStream;

    #[tokio::test]
    async fn local_ack_stream_should_have_a_closed_remote() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, ()>(Value::Str("local".into(), None)))
        });
        let empty_remote = futures_util::stream::empty::<()>();
        let stream: AckStream<_, _, (), ()> = AckStream::new_empty_remote(local, empty_remote, |_| unreachable!());
        futures_util::pin_mut!(stream);
        assert_eq!(stream.ack_cnt, 0);
        assert_eq!(stream.total_ack_cnt, 0);
        assert_eq!(stream.serv_cnt, 0);
        let data = stream.next().await;
        assert!(
            matches!(data, Some((id, Ok(Value::Str(msg, None)))) if id == sid && msg == "local")
        );
        assert_eq!(stream.next().await, None);
        assert!(stream.is_terminated());
    }
}
