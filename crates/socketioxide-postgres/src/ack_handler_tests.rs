//! `broadcast_with_ack` registers a response handler before publishing its request.
//! The handler must be removed when the request can't be sent, otherwise each failed
//! publish (e.g. during a backend outage) leaks one entry for the lifetime of the namespace.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::{FusedStream, Stream};
use serde::{Deserialize, Serialize};
use socketioxide_core::{
    Sid, Str, Uid, Value,
    adapter::errors::SocketError,
    adapter::{
        BroadcastFlags, BroadcastIter, BroadcastOptions, CoreAdapter, CoreLocalAdapter,
        RemoteSocketData, SocketEmitter,
    },
    packet::Packet,
    parser::Parse,
};
use socketioxide_parser_common::CommonParser;

use super::*;

struct StubEmitter(Str);
struct StubAckStream;
impl Stream for StubAckStream {
    type Item = (Sid, Result<Value, StubAckError>);
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}
impl FusedStream for StubAckStream {
    fn is_terminated(&self) -> bool {
        true
    }
}
#[derive(Debug, Serialize, Deserialize)]
struct StubAckError;
impl fmt::Display for StubAckError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}
impl std::error::Error for StubAckError {}

impl SocketEmitter for StubEmitter {
    type AckError = StubAckError;
    type AckStream = StubAckStream;
    fn get_all_sids(&self, _: impl Fn(&Sid) -> bool) -> Vec<Sid> {
        vec![]
    }
    fn get_remote_sockets(&self, _: BroadcastIter<'_>) -> Vec<RemoteSocketData> {
        vec![]
    }
    fn send_many(&self, _: BroadcastIter<'_>, _: Value) -> Result<(), Vec<SocketError>> {
        Ok(())
    }
    fn send_many_volatile(&self, _: BroadcastIter<'_>, _: Value) {}
    fn send_many_with_ack(
        &self,
        _: BroadcastIter<'_>,
        _: Packet,
        _: Option<Duration>,
    ) -> (Self::AckStream, u32) {
        (StubAckStream, 0)
    }
    fn disconnect_many(&self, _: Vec<Sid>) -> Result<(), Vec<SocketError>> {
        Ok(())
    }
    fn path(&self) -> &Str {
        &self.0
    }
    fn parser(&self) -> impl Parse {
        CommonParser
    }
    fn server_id(&self) -> Uid {
        Uid::ZERO
    }
    fn ack_timeout(&self) -> Duration {
        Duration::from_secs(5)
    }
}

/// What the driver does when the adapter sends a request.
#[derive(Debug, Clone, Copy)]
enum Mode {
    Success,
    Fail,
    Hang,
}

impl Mode {
    async fn run(self) -> Result<(), std::io::Error> {
        match self {
            Mode::Success => Ok(()),
            Mode::Fail => Err(std::io::Error::other("backend is down")),
            Mode::Hang => std::future::pending().await,
        }
    }
}

struct StubNotification;
impl Notification for StubNotification {
    fn channel(&self) -> &str {
        ""
    }
    fn payload(&self) -> &str {
        ""
    }
}

#[derive(Clone)]
struct StubDriver(Mode);
impl Driver for StubDriver {
    type Error = std::io::Error;
    type Notification = StubNotification;
    type NotificationStream = futures_util::stream::Empty<StubNotification>;

    async fn init(&self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn listen(&self, _: &[&str]) -> Result<Self::NotificationStream, Self::Error> {
        Ok(futures_util::stream::empty())
    }
    async fn notify(&self, _: &str, _: &str) -> Result<(), Self::Error> {
        self.0.run().await
    }
    async fn push_attachment(&self, _: &str, _: &[u8]) -> Result<i64, Self::Error> {
        Ok(0)
    }
    async fn get_attachment(&self, _: &str, _: i64) -> Result<Vec<u8>, Self::Error> {
        Ok(vec![])
    }
    async fn cleanup_attachments(&self, _: &str, _: Duration) -> Result<(), Self::Error> {
        Ok(())
    }
    async fn close(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn adapter(mode: Mode) -> CustomPostgresAdapter<StubEmitter, StubDriver> {
    let state =
        PostgresAdapterCtr::new_with_driver(StubDriver(mode), PostgresAdapterConfig::default());
    CoreAdapter::new(&state, CoreLocalAdapter::new(StubEmitter(Str::from("/"))))
}

fn packet() -> Packet {
    Packet::event(Str::from("/"), Value::Str(Str::from("[\"event\"]"), None))
}

fn opts() -> BroadcastOptions {
    let mut opts = BroadcastOptions::default();
    opts.add_flag(BroadcastFlags::Broadcast);
    opts
}

#[tokio::test]
async fn failed_request_removes_the_response_handler() {
    let adapter = adapter(Mode::Fail);
    let res = adapter.broadcast_with_ack(packet(), opts(), None).await;
    assert!(matches!(res, Err(Error::Driver(_))));
    assert!(adapter.responses.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_request_removes_the_response_handler() {
    let adapter = adapter(Mode::Hang);
    let fut = adapter.broadcast_with_ack(packet(), opts(), None);
    let res = tokio::time::timeout(Duration::from_millis(10), fut).await;
    assert!(res.is_err());
    assert!(adapter.responses.lock().unwrap().is_empty());
}

#[tokio::test]
async fn response_handler_lives_as_long_as_the_stream() {
    let adapter = adapter(Mode::Success);
    let stream = adapter
        .broadcast_with_ack(packet(), opts(), None)
        .await
        .map_err(|_| "broadcast should succeed")
        .unwrap();
    assert_eq!(adapter.responses.lock().unwrap().len(), 1);
    drop(stream);
    assert!(adapter.responses.lock().unwrap().is_empty());
}
