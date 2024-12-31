use std::{
    fmt,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use futures_core::{FusedStream, Stream};
use futures_util::{stream::TakeUntil, StreamExt};
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;
use socketioxide_core::adapter::AckStreamItem;
use tokio::time;

use crate::{
    drivers::{Driver, MessageStream},
    request::{Response, ResponseType},
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
    pub struct AckStream<S, D: Driver> {
        #[pin]
        local: S,
        #[pin]
        remote: DropStream<TakeUntil<MessageStream, time::Sleep>, D>,
        ack_cnt: u32,
        total_ack_cnt: usize,
        serv_cnt: u16,
    }
}

impl<S, D: Driver> AckStream<S, D> {
    pub fn new(
        local: S,
        remote: MessageStream,
        timeout: Duration,
        serv_cnt: u16,
        chan: String,
        driver: D,
    ) -> Self {
        let remote = remote.take_until(time::sleep(timeout));
        let remote = DropStream::new(remote, driver, chan);
        Self {
            local,
            remote,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt,
        }
    }
    pub fn new_local(local: S, driver: D) -> Self {
        let remote = MessageStream::new_empty().take_until(time::sleep(Duration::ZERO));
        let remote = DropStream::new(remote, driver, String::new());
        Self {
            local,
            remote,
            ack_cnt: 0,
            total_ack_cnt: 0,
            serv_cnt: 0,
        }
    }

    /// Poll the remote stream. First the count of acks is received, then the acks are received.
    /// We expect `serv_cnt` of `BroadcastAckCount` messages to be received, then we expect
    /// `ack_cnt` of `BroadcastAck` messages.
    fn poll_remote<E: DeserializeOwned + fmt::Debug>(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<AckStreamItem<E>>> {
        let projection = self.as_mut().project();
        match projection.remote.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(item)) => {
                let res = rmp_serde::from_slice::<Response<E>>(&item);
                match res {
                    Ok(Response {
                        uid,
                        req_id,
                        r#type: ResponseType::BroadcastAckCount(count),
                    }) if *projection.serv_cnt > 0 => {
                        tracing::trace!(?uid, ?req_id, "receiving broadcast ack count {count}");
                        *projection.ack_cnt += count;
                        *projection.total_ack_cnt += count as usize;
                        *projection.serv_cnt -= 1;
                        self.poll_remote(cx)
                    }
                    Ok(Response {
                        uid,
                        req_id,
                        r#type: ResponseType::BroadcastAck((sid, res)),
                    }) if *projection.ack_cnt > 0 => {
                        tracing::trace!(?uid, ?req_id, "receiving broadcast ack {sid} {:?}", res);
                        *projection.ack_cnt -= 1;
                        Poll::Ready(Some((sid, res)))
                    }
                    Ok(Response { uid, req_id, .. }) => {
                        tracing::warn!(?uid, ?req_id, ?self, "unexpected response type");
                        self.poll_remote(cx)
                    }
                    Err(e) => {
                        tracing::warn!("error decoding ack response: {e}");
                        self.poll_remote(cx)
                    }
                }
            }
        }
    }
}
impl<E, S, D> Stream for AckStream<S, D>
where
    E: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<E>>,
    D: Driver,
{
    type Item = AckStreamItem<E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project().local.poll_next(cx) {
            Poll::Pending | Poll::Ready(None) => self.poll_remote(cx),
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, upper) = self.local.size_hint();
        (lower, upper.map(|upper| upper + self.total_ack_cnt))
    }
}

impl<Err, S, D> FusedStream for AckStream<S, D>
where
    Err: DeserializeOwned + fmt::Debug,
    S: Stream<Item = AckStreamItem<Err>> + FusedStream,
    D: Driver,
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
impl<S, D: Driver> fmt::Debug for AckStream<S, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AckStream")
            .field("ack_cnt", &self.ack_cnt)
            .field("total_ack_cnt", &self.total_ack_cnt)
            .field("serv_cnt", &self.serv_cnt)
            .finish()
    }
}

pin_project! {
    /// A stream that unsubscribes from its source channel when dropped.
    pub struct DropStream<S, D: Driver> {
        #[pin]
        stream: S,
        driver: D,
        chan: String,
    }
    impl<S, D: Driver> PinnedDrop for DropStream<S, D> {
        fn drop(this: Pin<&mut Self>) {
            let stream = this.project();
            let driver = stream.driver.unsubscribe(std::mem::take(stream.chan));
            // TODO: Use AsyncDrop when stable
            tokio::spawn(async move {
                if let Err(e) = driver.await {
                    tracing::warn!("error unsubscribing from ack stream: {e}");
                }
            });
        }
    }
}
impl<S, D: Driver> DropStream<S, D> {
    pub fn new(stream: S, driver: D, chan: String) -> Self {
        Self {
            stream,
            driver,
            chan,
        }
    }
}
impl<S, D> Stream for DropStream<S, D>
where
    S: Stream,
    D: Driver,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}
impl<S, D> FusedStream for DropStream<S, D>
where
    S: FusedStream,
    D: Driver,
{
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}
