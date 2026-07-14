//! Polling mechanism tests.
//!
//! These tests exercise the [`Client`] read/write halves obtained via
//! [`Client::split`]: packets sent through the sink must reach the server and
//! the echoed packets must be surfaced back through the stream.

use bytes::Bytes;
use engineioxide_client::{Client, EioEvent};
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service};

mod fixture;

/// Packets sent through the write half must reach the server, and the packets
/// the handler echoes back must be surfaced in order through the read half.
#[tokio::test]
async fn round_trip() {
    let (svc, mut rx) = service();
    let client = Client::connect_polling(svc).await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    let (mut ctx, mut crx) = client.split::<EioEvent>();

    ctx.send(EioEvent::Message("Hello".into())).await.unwrap();
    ctx.send(EioEvent::Binary(Bytes::from_static(b"Hello")))
        .await
        .unwrap();

    // The server observes both packets.
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "Hello".into())
    );
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Binary(sid, Bytes::from_static(b"Hello"))
    );

    // And echoes them back through the read half, in order.
    match crx.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    match crx.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
}

/// Packets sent through the write half must reach the server, and the packets
/// the handler echoes back must be surfaced in order through the read half.
#[tokio::test]
async fn round_trip_ws() {
    let (svc, mut rx) = service();
    let client = Client::connect_ws(svc).await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    let (mut ctx, mut crx) = client.split::<EioEvent>();

    ctx.send(EioEvent::Message("Hello".into())).await.unwrap();
    ctx.send(EioEvent::Binary(Bytes::from_static(b"Hello")))
        .await
        .unwrap();

    // The server observes both packets.
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "Hello".into())
    );
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Binary(sid, Bytes::from_static(b"Hello"))
    );

    // And echoes them back through the read half, in order.
    match crx.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    match crx.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
}
