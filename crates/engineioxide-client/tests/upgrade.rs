//! Transport upgrade tests.
//!
//! Reference behavior (engine.io-client, protocol v4):
//! * After the handshake, if the server offers `websocket` in `upgrades` and
//!   the client is configured for it, the client probes: it connects a
//!   websocket with `EIO=4&transport=websocket&sid=...`, sends `2probe`,
//!   expects `3probe` back, pauses polling, then confirms with `5`.
//! * Polling stays active while probing: packets keep flowing until the
//!   probe succeeds, and nothing may be lost across the switch.
//! * **A failed probe never kills the session**: the client emits an
//!   `upgradeError` and keeps running on polling. There is no retry.
//! * No probe is attempted when the server offers no upgrade or when the
//!   client is not configured for websocket.

use bytes::Bytes;
use engineioxide_client::{Client, EioEvent};
use engineioxide_core::{Packet, TransportType};
use futures_util::{SinkExt, StreamExt};
use http::Method;

use crate::fixture::{Event, service, service_with_registry};

mod fixture;
mod mock;

#[tokio::test]
async fn upgrade() {
    let (svc, mut rx) = service();
    let mut client = Client::connect(svc, "localhost/engine.io").await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    assert_eq!(
        client.next().await.unwrap().unwrap(),
        EioEvent::Connect(sid)
    );

    assert_eq!(
        client.next().await.unwrap().unwrap(),
        EioEvent::Upgrade(TransportType::Websocket)
    );

    assert_eq!(client.transport(), TransportType::Websocket);
    client
        .send(EioEvent::Message("Hello".into()))
        .await
        .unwrap();
    client
        .send(EioEvent::Binary(Bytes::from_static(b"Hello")))
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
    match client.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    match client.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
}

/// Wire-level probe sequence: ws connect with the session `sid`, `2probe`
/// out, `3probe` in, `5` out. After the upgrade all traffic flows over the
/// websocket.
#[tokio::test]
async fn upgrade_probe_wire_sequence() {
    let open = mock::open_packet();
    let sid = open.sid.to_string();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let call = server.next_ws_parking_http().await;
        assert_eq!(call.query("EIO"), Some("4"));
        assert_eq!(call.query("transport"), Some("websocket"));
        assert_eq!(
            call.query("sid").map(str::to_owned),
            Some(sid),
            "the probe must join the existing session"
        );
        let mut ws = call.accept();
        assert_eq!(
            ws.recv_packet().await,
            Packet::PingUpgrade,
            "probe ping expected"
        );
        ws.send_packet(Packet::PongUpgrade);
        assert_eq!(
            ws.recv_packet().await,
            Packet::Upgrade,
            "upgrade confirmation expected"
        );
        ws
    };
    let (event, mut ws) = tokio::join!(mock::within("upgrade event", client.next()), script);
    assert_eq!(
        event.unwrap().unwrap(),
        EioEvent::Upgrade(TransportType::Websocket)
    );
    assert_eq!(client.transport(), TransportType::Websocket);

    // Traffic now flows over the websocket, in both directions.
    mock::within(
        "send over ws",
        client.send(EioEvent::Message("hello".into())),
    )
    .await
    .unwrap();
    assert_eq!(ws.recv_packet().await, Packet::Message("hello".into()));
    ws.send_packet(Packet::Message("world".into()));
    assert_eq!(
        mock::within("ws message", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Message("world".into())
    );
}

/// Polling must remain active while the probe is in flight: a message
/// arriving on the held poll during the probe must be delivered.
#[tokio::test]
async fn polling_stays_active_during_probe() {
    let open = mock::open_packet();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        // Wait for the probe connect, keeping track of the in-flight poll.
        let mut held_poll = None;
        let ws = loop {
            match server.next_call().await {
                mock::ServerCall::Ws(c) => break c,
                mock::ServerCall::Http(c) => held_poll = Some(c),
            }
        };
        let mut ws = ws.accept();
        assert_eq!(ws.recv_packet().await, Packet::PingUpgrade);

        // Mid-probe, the server delivers a message on the polling transport:
        // the reference client processes it (polling is only paused *after*
        // the probe succeeds).
        let poll = match held_poll {
            Some(c) => c,
            None => server.next_http().await,
        };
        poll.respond_packets([Packet::Message("during-probe".into())]);
        ws
    };

    // The client must yield the polled message even though it is probing.
    let (event, ws) = tokio::join!(
        mock::within("message delivered during probe", client.next()),
        script,
    );
    assert_eq!(
        event.unwrap().unwrap(),
        EioEvent::Message("during-probe".into())
    );

    // And the upgrade must still complete afterwards.
    let mut ws = ws;
    let finish = async {
        ws.send_packet(Packet::PongUpgrade);
        // A pausing client waits for its in-flight poll to complete: the
        // server releases it with a noop (reference server behavior).
        loop {
            tokio::select! {
                p = ws.recv_packet() => {
                    assert_eq!(p, Packet::Upgrade);
                    break;
                }
                call = server.next_call() => match call {
                    mock::ServerCall::Http(c) => c.respond_packets([Packet::Noop]),
                    mock::ServerCall::Ws(c) => {
                        panic!("unexpected second ws connect: {:?}", c.req)
                    }
                },
            }
        }
    };
    let (event, _) = tokio::join!(
        mock::within("upgrade event", async {
            match client.next().await.unwrap().unwrap() {
                EioEvent::Upgrade(t) => t,
                ev => panic!("unexpected event while finishing the probe: {ev:?}"),
            }
        }),
        finish,
    );
    assert_eq!(event, TransportType::Websocket);
}

/// A probe that cannot even connect must not kill the session: the client
/// keeps running on polling (official behavior: `upgradeError` event, no
/// close, no retry).
#[tokio::test]
async fn failed_ws_connect_falls_back_to_polling() {
    let open = mock::open_packet();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let mut held_poll = None;
        let ws = loop {
            match server.next_call().await {
                mock::ServerCall::Ws(c) => break c,
                mock::ServerCall::Http(c) => held_poll = Some(c),
            }
        };
        ws.reject("connection refused");
        // The session must keep working over polling.
        let poll = match held_poll {
            Some(c) => c,
            None => server.next_http().await,
        };
        poll.respond_packets([Packet::Message("still-alive".into())]);
    };

    let (event, _) = tokio::join!(
        mock::within("message after failed probe", async {
            match client.next().await {
                Some(Ok(EioEvent::Message(msg))) => msg,
                Some(Ok(ev)) => panic!("unexpected event after failed probe: {ev:?}"),
                Some(Err(e)) => panic!("a failed probe must not surface an error: {e}"),
                None => panic!("a failed probe must not close the session"),
            }
        }),
        script,
    );
    assert_eq!(event, "still-alive");
    assert_eq!(client.transport(), TransportType::Polling);
}

/// A probe answered with the wrong packet (a plain pong instead of
/// `3probe`) must abort the upgrade and keep the session on polling.
#[tokio::test]
async fn wrong_probe_reply_falls_back_to_polling() {
    let open = mock::open_packet();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let mut held_poll = None;
        let ws = loop {
            match server.next_call().await {
                mock::ServerCall::Ws(c) => break c,
                mock::ServerCall::Http(c) => held_poll = Some(c),
            }
        };
        let mut ws = ws.accept();
        assert_eq!(ws.recv_packet().await, Packet::PingUpgrade);
        ws.send_packet(Packet::Pong); // wrong reply: plain pong, not "3probe"
        let poll = match held_poll {
            Some(c) => c,
            None => server.next_http().await,
        };
        poll.respond_packets([Packet::Message("still-alive".into())]);
        ws
    };

    let (msg, _ws) = tokio::join!(
        mock::within("message after failed probe", async {
            match client.next().await {
                Some(Ok(EioEvent::Message(msg))) => msg,
                Some(Ok(ev)) => panic!("unexpected event after failed probe: {ev:?}"),
                Some(Err(e)) => panic!("a failed probe must not surface an error: {e}"),
                None => panic!("a failed probe must not close the session"),
            }
        }),
        script,
    );
    assert_eq!(msg, "still-alive");
    assert_eq!(client.transport(), TransportType::Polling);
}

/// A websocket closed mid-probe must abort the upgrade and keep the session
/// on polling.
#[tokio::test]
async fn ws_closed_during_probe_falls_back_to_polling() {
    let open = mock::open_packet();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let mut held_poll = None;
        let ws = loop {
            match server.next_call().await {
                mock::ServerCall::Ws(c) => break c,
                mock::ServerCall::Http(c) => held_poll = Some(c),
            }
        };
        let mut ws = ws.accept();
        assert_eq!(ws.recv_packet().await, Packet::PingUpgrade);
        ws.close(); // the probe transport dies before answering
        let poll = match held_poll {
            Some(c) => c,
            None => server.next_http().await,
        };
        poll.respond_packets([Packet::Message("still-alive".into())]);
    };

    let (msg, _) = tokio::join!(
        mock::within("message after failed probe", async {
            match client.next().await {
                Some(Ok(EioEvent::Message(msg))) => msg,
                Some(Ok(ev)) => panic!("unexpected event after failed probe: {ev:?}"),
                Some(Err(e)) => panic!("a failed probe must not surface an error: {e}"),
                None => panic!("a failed probe must not close the session"),
            }
        }),
        script,
    );
    assert_eq!(msg, "still-alive");
    assert_eq!(client.transport(), TransportType::Polling);
}

/// No probe may be attempted when the server offers no upgrade.
#[tokio::test]
async fn no_probe_when_server_offers_no_upgrade() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) =
        mock::connect_polling(&open, [TransportType::Polling, TransportType::Websocket]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let poll = match server.next_call().await {
            mock::ServerCall::Http(c) => c,
            mock::ServerCall::Ws(c) => {
                panic!(
                    "no ws connect expected without server upgrades: {:?}",
                    c.req
                )
            }
        };
        assert_eq!(poll.method, Method::GET);
        server
            .assert_no_call(
                std::time::Duration::from_millis(300),
                "no upgrade is offered",
            )
            .await;
        poll.respond_packets([Packet::Message("plain-polling".into())]);
    };
    let (event, _) = tokio::join!(mock::within("polled message", client.next()), script);
    assert_eq!(
        event.unwrap().unwrap(),
        EioEvent::Message("plain-polling".into())
    );
    assert_eq!(client.transport(), TransportType::Polling);
}

/// No probe may be attempted when the client is configured for polling only,
/// even if the server offers the websocket upgrade.
#[tokio::test]
async fn no_probe_when_client_is_polling_only() {
    let open = mock::open_packet(); // server offers websocket
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let script = async {
        let poll = match server.next_call().await {
            mock::ServerCall::Http(c) => c,
            mock::ServerCall::Ws(c) => {
                panic!(
                    "no ws connect expected for a polling-only client: {:?}",
                    c.req
                )
            }
        };
        server
            .assert_no_call(
                std::time::Duration::from_millis(300),
                "client is polling-only",
            )
            .await;
        poll.respond_packets([Packet::Message("plain-polling".into())]);
    };
    let (event, _) = tokio::join!(mock::within("polled message", client.next()), script);
    assert_eq!(
        event.unwrap().unwrap(),
        EioEvent::Message("plain-polling".into())
    );
    assert_eq!(client.transport(), TransportType::Polling);
}

/// A message sent while the upgrade is in progress must be buffered and
/// delivered once the new transport is live (the reference client inhibits
/// `flush()` while `upgrading` and flushes right after the upgrade).
#[tokio::test]
async fn send_during_upgrade_is_delivered_after_upgrade() {
    let (svc, mut rx) = service();
    let client = Client::connect(svc, "localhost/engine.io").await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    let (mut ctx, mut crx) = client.split::<EioEvent>();
    assert_eq!(crx.next().await.unwrap().unwrap(), EioEvent::Connect(sid));

    // The send blocks until the upgrade completes, so it must be driven
    // concurrently with the stream.
    tokio::join!(
        async {
            fixture::within(
                "send during upgrade",
                ctx.send(EioEvent::Message("buffered".into())),
            )
            .await
            .unwrap();
        },
        async {
            assert_eq!(
                fixture::within("upgrade event", crx.next())
                    .await
                    .unwrap()
                    .unwrap(),
                EioEvent::Upgrade(TransportType::Websocket)
            );
            assert_eq!(
                fixture::within("echo after upgrade", crx.next())
                    .await
                    .unwrap()
                    .unwrap(),
                EioEvent::Message("buffered".into()),
                "the buffered message must be delivered once upgraded"
            );
        },
    );

    assert_eq!(
        fixture::within("server side message", rx.recv())
            .await
            .unwrap(),
        Event::Message(sid, "buffered".into())
    );
}

/// A message emitted by the server right at connection time must not be lost
/// even though the client immediately upgrades to websocket.
#[tokio::test]
async fn no_message_loss_across_upgrade() {
    let (svc, mut rx, registry) = service_with_registry(Default::default());
    let mut client = Client::connect(svc, "localhost/engine.io").await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));

    // Emit while the client is (most likely) still on polling.
    registry.lock().unwrap()[&sid].emit("early").unwrap();

    let received = fixture::within("early message", async {
        loop {
            match client.next().await {
                Some(Ok(EioEvent::Message(msg))) => break msg,
                Some(Ok(_)) => continue, // Connect / Upgrade events
                Some(Err(e)) => panic!("client stream error: {e}"),
                None => panic!("stream ended before the early message"),
            }
        }
    })
    .await;
    assert_eq!(received, "early");
}
