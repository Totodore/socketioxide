use std::{
    borrow::Cow, fmt, pin::Pin, sync::{Arc, Mutex}, task::{self, Poll}, time::Duration
};

use futures_core::{FusedStream, Stream, future::BoxFuture, ready};
use futures_util::{StreamExt, stream::TakeUntil};
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use socketioxide_core::{
    Sid,
    adapter::{
        AckStreamItem,
        remote_packet::{Response, ResponseType},
    },
};
use tokio::{sync::mpsc, time};

use crate::{
    ResponseHandlers, ResponsePayload,
    drivers::{Driver, Notification},
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
        remote: DropStream<TakeUntil<ChanStream<Box<RawValue>>, time::Sleep>>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<S> AckStream<S> {
    pub fn new(
        local: S,
        remote: mpsc::Receiver<Box<RawValue>>,
        timeout: Duration,
        serv_cnt: u16,
        req_sid: Sid,
        handlers: Arc<Mutex<ResponseHandlers>>,
    ) -> Self {
        let remote = ChanStream::new(remote).take_until(time::sleep(timeout));
        let remote = DropStream::new(remote, handlers, req_sid);
        Self {
            local,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt,
            remote,
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
    /// Poll the remote stream. First the count of acks is receivedhen the acks are received.
    /// We expect `serv_cnt` of `BroadcastAckCount` messages to be receivedhen we expect
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
                    let res = serde_json::from_str::<Response<E>>(notif.get());
                    match res {
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
impl<S: Notification> fmt::Debug for AckStream<S> {
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
    pub struct ChanStream<T> {
        #[pin]
        rx: mpsc::Receiver<T>
    }
}
impl<T> ChanStream<T> {
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

pin_project! {
    struct RemoteAckStream<D: Driver> {
        driver: D,
        table: Cow<'static, str>,
        #[pin]
        inner: ChanStream<ResponsePayload>,
        #[pin]
        state: RemoteAckStreamState<Box<dyn Future<Output = Box<RawValue>> + 'static>>,
    }
}

pin_project! {
    #[project = RemoteAckStreamStateProj]
    enum RemoteAckStreamState {
        Pending{ #[pin] fut: Box<dyn Future<Output = Result<Box<RawValue>, Error>> + 'static> },
        Done,
    }
}

impl<D: Driver> Stream for RemoteAckStream<D> {
    type Item = Box<RawValue>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        let proj = self.project();
        match proj.state.project() {
            RemoteAckStreamStateProj::Pending { fut } => match ready!(fut.poll(cx)) {
                Ok(value) => {
                    proj.state.set(RemoteAckStreamState::Done);
                    cx.waker().wake_by_ref();
                    return Poll::Ready(Some(value))
                },
                Err(err) => Poll::Ready(Some(Box::new(Value::String(err.to_string())))),
            },
            RemoteAckStreamStateProj::Done => (),
        };

        match ready!(self.project().inner.poll_next(cx)) {
            Some(ResponsePayload::Data(data)) => Poll::Ready(Some(data)),
            Some(ResponsePayload::Attachment(id)) => self.driver.get_attachment(&self.table, id)
            None => Poll::Ready(None),
        }
    }
}
impl<D: Driver> FusedStream for RemoteAckStream<D> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
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
        let stream = AckStream::<_>::new_local(local);
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
