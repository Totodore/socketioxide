//! Reconnection behavior tests.
//!
//! Reference behavior: **engine.io-client never reconnects on its own** —
//! reconnection (backoff, retries) belongs to the layer above (socket.io's
//! `Manager`). After any close the client goes fully silent: no request may
//! ever be issued again on that instance. Reconnecting means creating a new
//! client, which starts a brand new session (new handshake, new sid).

use std::time::Duration;

use engineioxide_client::{Client, EioEvent, EngineIoClientConfig};
use engineioxide_core::{Packet, TransportType};
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service};

mod fixture;
mod mock;

/// After a server-initiated close, the client must go silent: no polling
/// request, no websocket connect, ever.
#[tokio::test]
async fn no_auto_reconnect_after_server_close() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (disconnect, _) = tokio::join!(mock::within("disconnect event", client.next()), async {
        server.next_http().await.respond_packets([Packet::Close])
    },);
    assert_eq!(disconnect.unwrap().unwrap(), EioEvent::Disconnect);
    assert!(mock::within("end of stream", client.next()).await.is_none());

    // Keep polling the terminated stream while watching for any request.
    tokio::join!(
        server.assert_no_call(Duration::from_millis(400), "the session is closed"),
        async {
            for _ in 0..3 {
                assert!(
                    client.next().await.is_none(),
                    "a terminated stream stays terminated"
                );
            }
        },
    );
}

/// After a transport error, the client must go silent as well: retrying is
/// the responsibility of the caller.
#[tokio::test]
async fn no_auto_reconnect_after_transport_error() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (err, _) = tokio::join!(mock::within("stream error", client.next()), async {
        server.next_http().await.respond(500, "")
    },);
    assert!(
        matches!(err, Some(Err(_))),
        "expected a transport error, got: {err:?}"
    );
    assert!(
        mock::within("stream termination", client.next())
            .await
            .is_none(),
        "the stream must terminate after a transport error"
    );

    server
        .assert_no_call(Duration::from_millis(400), "the session errored out")
        .await;
}

/// Reconnecting is done by creating a new client: it performs a fresh
/// handshake and gets a brand new session.
#[tokio::test]
async fn manual_reconnect_creates_a_new_session() {
    let (svc, mut rx) = service();
    let config = EngineIoClientConfig::builder()
        .transports([TransportType::Polling])
        .build();

    let mut first = Client::connect(svc.clone(), config)
        .await
        .unwrap();
    let first_sid = first.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(first_sid));
    assert_eq!(
        first.next().await.unwrap().unwrap(),
        EioEvent::Connect(first_sid)
    );
    fixture::within("close first client", first.close())
        .await
        .expect("close must succeed");
    drop(first);

    // A new client establishes a brand new session on the same server.
    let config = EngineIoClientConfig::builder()
        .transports([TransportType::Polling])
        .build();
    let mut second = Client::connect(svc, config).await.unwrap();
    let second_sid = second.sid();
    assert_ne!(second_sid, first_sid, "a reconnection is a new session");
    assert_eq!(
        second.next().await.unwrap().unwrap(),
        EioEvent::Connect(second_sid)
    );

    // The new session is fully functional.
    second
        .send(EioEvent::Message("again".into()))
        .await
        .unwrap();
    assert_eq!(
        fixture::within("echo on the new session", second.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Message("again".into())
    );
}
