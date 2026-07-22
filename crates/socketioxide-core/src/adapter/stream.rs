//! Shared stream constructs ([`AckStream`], [`DropStream`], [`ResponseHandlers`])
//! for remote adapters.

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

use crate::adapter::SocketEmitter;

/// A map of request IDs to response senders, used to route responses to the correct awaiting stream.
pub type ResponseHandlers<T> = HashMap<Sid, mpsc::Sender<T>>;

/// Decoder function that converts a raw remote payload item into a [`Response`].
pub type AckDecoder<I, E> = fn(I) -> Result<Response<E>, Box<dyn std::error::Error + Send>>;

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
    pub struct AckStream<E: SocketEmitter, R: Stream, T> {
        #[pin]
        local: E::AckStream,
        #[pin]
        remote: DropStream<TakeUntil<R, time::Sleep>, T>,
        decode: AckDecoder<R::Item, E::AckError>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<E: SocketEmitter, R: Stream, T> AckStream<E, R, T> {
    /// Create a new `AckStream` wrapping the given local and remote streams.
    pub fn new(
        local: E::AckStream,
        remote: R,
        decode: AckDecoder<R::Item, E::AckError>,
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
}

impl<E: SocketEmitter, R: Stream, T> AckStream<E, R, T>
where
    E::AckError: DeserializeOwned + fmt::Debug,
{
    fn poll_remote(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<E::AckError>>> {
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

impl<E: SocketEmitter, R: Stream, T> Stream for AckStream<E, R, T>
where
    E::AckError: DeserializeOwned + fmt::Debug,
{
    type Item = AckStreamItem<E::AckError>;

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

impl<E: SocketEmitter, R: Stream, T> FusedStream for AckStream<E, R, T>
where
    E::AckError: DeserializeOwned + fmt::Debug,
{
    fn is_terminated(&self) -> bool {
        let remote_term = (self.ack_cnt == 0 && self.serv_cnt == 0) || self.remote.is_terminated();
        self.local.is_terminated() && remote_term
    }
}

impl<E: SocketEmitter, R: Stream, T> fmt::Debug for AckStream<E, R, T> {
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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::adapter::errors::SocketError;
    use crate::adapter::remote_packet::{Response, ResponseType};
    use crate::adapter::{AckStreamItem, BroadcastIter, RemoteSocketData, SocketEmitter};
    use crate::packet::Packet;
    use crate::parser::{Parse, test::StubParser};
    use crate::{Sid, Str, Uid, Value};
    use futures_core::{FusedStream, Stream};
    use futures_util::StreamExt;
    use serde::{Deserialize, Serialize};

    use super::AckStream;

    type MockError = Box<dyn std::error::Error + Send>;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestError;
    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "test error")
        }
    }
    impl std::error::Error for TestError {}

    struct TestEmitter<S>(pub S);

    impl<S> SocketEmitter for TestEmitter<S>
    where
        S: Stream<Item = AckStreamItem<TestError>> + FusedStream + Send + Sync + 'static,
    {
        type AckError = TestError;
        type AckStream = S;

        fn get_all_sids(&self, _filter: impl Fn(&Sid) -> bool) -> Vec<Sid> {
            vec![]
        }
        fn get_remote_sockets(&self, _sids: BroadcastIter<'_>) -> Vec<RemoteSocketData> {
            vec![]
        }
        fn send_many(
            &self,
            _sids: BroadcastIter<'_>,
            _data: Value,
        ) -> Result<(), Vec<SocketError>> {
            Ok(())
        }
        fn send_many_volatile(&self, _sids: BroadcastIter<'_>, _data: Value) {}
        fn send_many_with_ack(
            &self,
            _sids: BroadcastIter<'_>,
            _packet: Packet,
            _timeout: Option<Duration>,
        ) -> (Self::AckStream, u32) {
            unreachable!()
        }
        fn disconnect_many(&self, _sids: Vec<Sid>) -> Result<(), Vec<SocketError>> {
            Ok(())
        }
        fn path(&self) -> &Str {
            unreachable!()
        }
        fn parser(&self) -> impl Parse {
            StubParser
        }
        fn server_id(&self) -> Uid {
            Uid::ZERO
        }
        fn ack_timeout(&self) -> Duration {
            Duration::from_secs(5)
        }
    }

    fn identity<E>(item: Result<Response<E>, MockError>) -> Result<Response<E>, MockError> {
        item
    }

    fn mock_ack_count(count: u32) -> Result<Response<TestError>, MockError> {
        Ok(Response {
            node_id: Uid::ZERO,
            r#type: ResponseType::BroadcastAckCount(count),
        })
    }

    fn mock_ack(sid: Sid, msg: &'static str) -> Result<Response<TestError>, MockError> {
        Ok(Response {
            node_id: Uid::ZERO,
            r#type: ResponseType::BroadcastAck((sid, Ok(Value::Str(msg.into(), None)))),
        })
    }

    fn mock_all_rooms() -> Result<Response<TestError>, MockError> {
        Ok(Response {
            node_id: Uid::ZERO,
            r#type: ResponseType::AllRooms(Default::default()),
        })
    }

    fn decode_error() -> Result<Response<TestError>, MockError> {
        Err(Box::new(std::io::Error::other("decode failure")))
    }

    #[tokio::test]
    async fn remote_broadcast_ack_count_updates_counters() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::once(async { mock_ack_count(3) });
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        assert_eq!(stream.serv_cnt, 1);
        let item = stream.next().await;
        assert!(item.is_none(), "BroadcastAckCount should not yield");
        assert_eq!(stream.serv_cnt, 0);
        assert_eq!(stream.ack_cnt, 3);
        assert_eq!(stream.total_ack_cnt, 3);
    }

    #[tokio::test]
    async fn remote_broadcast_ack_yields_item() {
        let sid = Sid::new();
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::iter([mock_ack_count(1), mock_ack(sid, "ack")]);
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let data = stream.next().await;
        assert!(matches!(data, Some((id, Ok(Value::Str(msg, None)))) if id == sid && msg == "ack"));
    }

    #[tokio::test]
    async fn mixed_local_and_remote_yields_local_first() {
        let local_sid = Sid::new();
        let remote_sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (
                local_sid,
                Ok::<_, TestError>(Value::Str("local".into(), None)),
            )
        });
        let remote =
            futures_util::stream::iter([mock_ack_count(1), mock_ack(remote_sid, "remote")]);
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let first = stream.next().await;
        assert!(
            matches!(first, Some((id, Ok(Value::Str(msg, None)))) if id == local_sid && msg == "local")
        );
        let second = stream.next().await;
        assert!(
            matches!(second, Some((id, Ok(Value::Str(msg, None)))) if id == remote_sid && msg == "remote")
        );
    }

    #[tokio::test]
    async fn unexpected_response_type_is_skipped() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::once(async { mock_all_rooms() });
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let result = stream.next().await;
        assert!(
            result.is_none(),
            "Unexpected response type should not yield"
        );
    }

    #[tokio::test]
    async fn decode_error_is_skipped() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::once(async { decode_error() });
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let result = stream.next().await;
        assert!(result.is_none(), "Decode error should not yield");
    }

    #[tokio::test]
    async fn terminated_when_counters_zero_and_local_exhausted() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::pending::<Result<Response<TestError>, MockError>>();
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            0,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        assert!(stream.is_terminated());
    }

    #[tokio::test]
    async fn size_hint_incorporates_remote_acks() {
        let sid = Sid::new();
        let local = futures_util::stream::once(async move {
            (sid, Ok::<_, TestError>(Value::Str("local".into(), None)))
        });
        let remote = futures_util::stream::empty();
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let mut stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            |_: Result<Response<TestError>, MockError>| unreachable!(),
            Duration::from_secs(5),
            0,
            Sid::new(),
            handlers,
        );
        stream.total_ack_cnt = 5;

        let (lower, upper) = stream.size_hint();
        assert_eq!(lower, 1);
        assert_eq!(upper, Some(6));
    }

    #[tokio::test]
    async fn broadcast_ack_not_yielded_when_ack_cnt_zero() {
        let sid = Sid::new();
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::once(async { mock_ack(sid, "ack") });
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let result = stream.next().await;
        assert!(
            result.is_none(),
            "BroadcastAck should not yield when ack_cnt==0"
        );
    }

    #[tokio::test]
    async fn multiple_broadcast_acks_delivered_in_order() {
        let sid1 = Sid::new();
        let sid2 = Sid::new();
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::iter([
            mock_ack_count(2),
            mock_ack(sid1, "first"),
            mock_ack(sid2, "second"),
        ]);
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            1,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let first = stream.next().await;
        assert!(
            matches!(first, Some((id, Ok(Value::Str(msg, None)))) if id == sid1 && msg == "first")
        );
        let second = stream.next().await;
        assert!(
            matches!(second, Some((id, Ok(Value::Str(msg, None)))) if id == sid2 && msg == "second")
        );
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn broadcast_ack_count_guard_ignores_when_serv_cnt_zero() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let remote = futures_util::stream::once(async { mock_ack_count(5) });
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            0,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        let result = stream.next().await;
        assert!(result.is_none());
        assert_eq!(stream.ack_cnt, 0);
    }

    #[tokio::test]
    async fn serv_cnt_decremented_for_each_server() {
        let local = futures_util::stream::empty::<AckStreamItem<TestError>>();
        let handlers = Arc::new(Mutex::new(super::ResponseHandlers::<TestError>::new()));
        let remote =
            futures_util::stream::iter([mock_ack_count(1), mock_ack_count(1), mock_ack_count(1)]);

        let stream: AckStream<TestEmitter<_>, _, TestError> = AckStream::new(
            local,
            remote,
            identity::<TestError>,
            Duration::from_secs(5),
            3,
            Sid::new(),
            handlers,
        );
        futures_util::pin_mut!(stream);

        assert_eq!(stream.serv_cnt, 3);
        let _ = stream.next().await;
        assert_eq!(stream.serv_cnt, 0);
        assert_eq!(stream.ack_cnt, 3);
        assert_eq!(stream.total_ack_cnt, 3);
    }
}
