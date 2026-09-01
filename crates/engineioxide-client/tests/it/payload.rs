//! Payload encoding and size-limit tests.
//!
//! Reference behavior (engine.io-client, protocol v4):
//! * Polling payloads are packets joined with the `\x1e` record separator;
//!   binary packets are base64 with a `b` prefix. Websocket sends binary
//!   data as raw binary frames.
//! * `maxPayload` (from the handshake) bounds the size of a multi-packet
//!   polling POST: the write buffer is split so each POST stays under the
//!   limit. A single packet bigger than the limit is sent anyway and the
//!   server rejects it (HTTP 413) — which is a transport error.

use bytes::Bytes;
use engineioxide::{TransportType, config::EngineIoConfig};
use engineioxide_client::{Client, EioEvent};
use engineioxide_core::Packet;
use futures_util::{SinkExt, StreamExt};

use crate::mock::{
    self,
    fixture::{Event, service, service_with_config},
    helpers::{ClientTestExt, FutureTestExt},
};

/// Several packets received in a single poll response (separated by `\x1e`)
/// must be surfaced in order.
#[tokio::test]
async fn multiple_packets_in_a_single_poll_response() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    let (events, _) = tokio::join!(
        async {
            let mut events = Vec::new();
            for _ in 0..3 {
                events.push(client.next().await.unwrap().unwrap());
            }
            events
        }
        .timeout(),
        async {
            server.next_http().await.respond_packets([
                Packet::Message("first".into()),
                Packet::Message("second".into()),
                Packet::Binary(Bytes::from_static(&[1, 2, 3])),
            ]);
        },
    );
    assert_eq!(
        events,
        vec![
            EioEvent::Message("first".into()),
            EioEvent::Message("second".into()),
            EioEvent::Binary(Bytes::from_static(&[1, 2, 3])),
        ]
    );
}

/// Packets queued before a flush must be batched in a single POST, joined
/// with the record separator.
#[tokio::test]
async fn flush_batches_packets_with_record_separator() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    client.feed(EioEvent::Message("a".into())).await.unwrap();
    client.feed(EioEvent::Message("b".into())).await.unwrap();
    client.feed(EioEvent::Message("c".into())).await.unwrap();

    let (res, _) = tokio::join!(client.flush().timeout(), async {
        let post = server.next_post_parking_get().await;
        assert_eq!(&post.body[..], b"4a\x1e4b\x1e4c");
        post.respond_ok();
    },);
    assert!(res.is_ok());
}

/// Binary packets on the polling transport are base64-encoded with a `b`
/// prefix, in both directions.
#[tokio::test]
async fn binary_is_base64_on_polling() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    // Inbound: `bAQID` is [1, 2, 3].
    let (event, _) = tokio::join!(client.next_ok().timeout(), async {
        server.next_http().await.respond(200, "bAQID")
    },);
    assert_eq!(event, EioEvent::Binary(Bytes::from_static(&[1, 2, 3])));

    // Outbound: [4, 5, 6] must be POSTed as `bBAUG`.
    let (res, _) = tokio::join!(
        client
            .send(EioEvent::Binary(Bytes::from_static(&[4, 5, 6])))
            .timeout(),
        async {
            let post = server.next_post_parking_get().await;
            assert_eq!(&post.body[..], b"bBAUG");
            post.respond_ok();
        },
    );
    assert!(res.is_ok());
}

/// A multi-packet flush must be split so that each polling POST stays under
/// the handshake `maxPayload` (reference `_getWritablePackets`). The server
/// enforces the limit with HTTP 413, so all four messages arriving proves
/// the client split the batch.
#[tokio::test]
async fn flush_splits_batches_at_max_payload() {
    let config = EngineIoConfig::builder().max_payload(100).build();
    let (svc, mut rx) = service_with_config(config);

    let mut client = Client::connect(svc, [TransportType::Polling])
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    // 4 messages of 30 bytes each: 127 wire bytes in a single payload,
    // which must be split into (at least) two POSTs of <= 100 bytes.
    let msg = "a".repeat(30);
    for _ in 0..4 {
        client
            .feed(EioEvent::Message(msg.clone().into()))
            .await
            .unwrap();
    }
    client.flush().timeout().await.unwrap();

    for i in 0..4 {
        assert_eq!(
            rx.next_ok().timeout().await,
            Event::Message(sid, msg.clone().into()),
            "message {i} must arrive: batches must be split under maxPayload"
        );
    }
}

/// A single packet over `maxPayload` cannot be split: the reference client
/// sends it anyway, the server rejects it (413) and the failure surfaces as
/// a transport error — never a panic.
#[tokio::test]
async fn oversized_packet_surfaces_a_transport_error() {
    let config = EngineIoConfig::builder().max_payload(100).build();
    let (svc, mut rx) = service_with_config(config);

    let mut client = Client::connect(svc, [TransportType::Polling])
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    let res = client
        .send(EioEvent::Message("a".repeat(300).into()))
        .timeout()
        .await; //TODO: correct equality
    assert!(
        res.is_err(),
        "an oversized write rejected by the server must surface an error"
    );
}

/// An empty message must round-trip unchanged.
#[tokio::test]
async fn empty_message_round_trip() {
    let (svc, mut rx) = service();
    let mut client = Client::connect(svc, [TransportType::Polling])
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    client.send(EioEvent::Message("".into())).await.unwrap();
    assert_eq!(rx.next_ok().timeout().await, Event::Message(sid, "".into()));
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Message("".into())
    );
}

/// A multibyte utf-8 message must round-trip unchanged on both transports.
#[tokio::test]
async fn utf8_message_round_trip_ws() {
    let (svc, mut rx) = service();
    let mut client = Client::connect(svc, [TransportType::Websocket])
        .timeout()
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    let text = "héllo 🌍 世界";
    client.send(EioEvent::Message(text.into())).await.unwrap();
    assert_eq!(
        rx.next_ok().timeout().await,
        Event::Message(sid, text.into()),
    );
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Message(text.into()),
    );
}
/// A multibyte utf-8 message must round-trip unchanged on both transports.
#[tokio::test]
async fn utf8_message_round_trip_polling() {
    let (svc, mut rx) = service();
    let mut client = Client::connect(svc, [TransportType::Polling])
        .timeout()
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    let text = "héllo 🌍 世界";
    client.send(EioEvent::Message(text.into())).await.unwrap();
    assert_eq!(
        rx.next_ok().timeout().await,
        Event::Message(sid, text.into()),
    );
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Message(text.into()),
    );
}

/// A large binary payload must round-trip unchanged over websocket.
#[tokio::test]
async fn large_binary_round_trip_ws() {
    let (svc, mut rx) = service();
    let mut client = Client::connect(svc, [TransportType::Websocket])
        .await
        .unwrap();
    let sid = client.sid();
    assert_eq!(rx.next_ok().timeout().await, Event::Connect(sid));
    assert_eq!(client.next_ok().timeout().await, EioEvent::Connect(sid));

    // 8KiB crosses the server's default 4KiB websocket read buffer.
    let data: Bytes = (0..8192u32)
        .map(|i| (i % 251) as u8)
        .collect::<Vec<_>>()
        .into();
    client
        .send(EioEvent::Binary(data.clone()))
        .timeout()
        .await
        .unwrap();
    assert_eq!(
        rx.next_ok().timeout().await,
        Event::Binary(sid, data.clone())
    );
    assert_eq!(client.next_ok().timeout().await, EioEvent::Binary(data));
}
