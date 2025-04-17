use std::{
    fmt,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{self, Poll},
    time::Duration,
};

use futures_core::{FusedStream, Stream};
use futures_util::{stream::TakeUntil, StreamExt};
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;
use socketioxide_core::{adapter::AckStreamItem, Sid};
use tokio::{sync::mpsc, time};

use crate::{
    drivers::Item,
    request::{Response, ResponseType},
    ResponseHandlers,
};

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
    pub struct AckStream<S> {
        #[pin]
        local: S,
        #[pin]
        remote: DropStream<TakeUntil<ChanStream, time::Sleep>>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<S> AckStream<S> {
    pub fn new(
        local: S,
        rx: mpsc::Receiver<Item>,
        timeout: Duration,
        serv_cnt: u16,
        req_id: Sid,
        handlers: Arc<Mutex<ResponseHandlers>>,
    ) -> Self {
        let remote = ChanStream::new(rx).take_until(time::sleep(timeout));
        let remote = DropStream::new(remote, handlers, req_id);
        Self {
            local,
            remote,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt,
        }
    }
    pub fn new_local(local: S) -> Self {
        let handlers = Arc::new(Mutex::new(ResponseHandlers::new()));
        let rx = mpsc::channel(1).1;
        let remote = ChanStream::new(rx).take_until(time::sleep(Duration::ZERO));
        let remote = DropStream::new(remote, handlers, Sid::ZERO);
        Self {
            local,
            remote,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt: 0,
        }
    }
}
impl<Err, S> AckStream<S>
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
                Poll::Ready(Some(Item { header, data, .. })) => {
                    let res = rmp_serde::from_slice::<Response<E>>(&data);
                    match res {
                        Ok(Response {
                            node_id: uid,
                            r#type: ResponseType::BroadcastAckCount(count),
                        }) if *projection.serv_cnt > 0 => {
                            tracing::trace!(?uid, ?header, "receiving broadcast ack count {count}");
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
                                ?header,
                                "receiving broadcast ack {sid} {:?}",
                                res
                            );
                            *projection.ack_cnt -= 1;
                            return Poll::Ready(Some((sid, res)));
                        }
                        Ok(Response { node_id: uid, .. }) => {
                            tracing::warn!(?uid, ?header, "unexpected response type");
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
impl<E, S> Stream for AckStream<S>
where
    E: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<E>> + FusedStream,
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

impl<Err, S> FusedStream for AckStream<S>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
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
impl<S> fmt::Debug for AckStream<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AckStream")
            .field("ack_cnt", &self.ack_cnt)
            .field("total_ack_cnt", &self.total_ack_cnt)
            .field("serv_cnt", &self.serv_cnt)
            .finish()
    }
}

pin_project! {
    /// A stream of messages received from a channel.
    pub struct ChanStream {
        #[pin]
        rx: mpsc::Receiver<Item>
    }
}
impl ChanStream {
    pub fn new(rx: mpsc::Receiver<Item>) -> Self {
        Self { rx }
    }
}
impl Stream for ChanStream {
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().rx.poll_recv(cx)
    }
}
pin_project! {
    /// A stream that unsubscribes from its source channel when dropped.
    pub struct DropStream<S> {
        #[pin]
        stream: S,
        req_id: Sid,
        handlers: Arc<Mutex<ResponseHandlers>>
    }
    impl<S> PinnedDrop for DropStream<S> {
        fn drop(this: Pin<&mut Self>) {
            let stream = this.project();
            let chan = stream.req_id;
            tracing::debug!(?chan, "dropping stream");
            stream.handlers.lock().unwrap().remove(chan);
        }
    }
}
impl<S> DropStream<S> {
    pub fn new(stream: S, handlers: Arc<Mutex<ResponseHandlers>>, req_id: Sid) -> Self {
        Self {
            stream,
            handlers,
            req_id,
        }
    }
}
impl<S: Stream> Stream for DropStream<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}
impl<S: FusedStream> FusedStream for DropStream<S> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}
