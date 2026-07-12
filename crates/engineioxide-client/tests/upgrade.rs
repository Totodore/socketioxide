use bytes::Bytes;
use engineioxide::TransportType;
use engineioxide_client::{Client, EioEvent};
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service};

mod fixture;

#[tokio::test]
async fn upgrade() {
    let (svc, mut rx) = service();
    let mut client = Client::connect_default(svc).await.unwrap();
    let sid = client.sid;
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));

    assert_eq!(
        client.next().await.unwrap().unwrap(),
        EioEvent::Upgrade(TransportType::Websocket)
    );

    assert_eq!(client.transport(), TransportType::Websocket);
    dbg!("a");
    client
        .send(EioEvent::Message("Hello".into()))
        .await
        .unwrap();
    dbg!("a");
    client
        .send(EioEvent::Binary(Bytes::from_static(b"Hello")))
        .await
        .unwrap();
    dbg!("a");

    // The server observes both packets.
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "Hello".into())
    );
    dbg!("a");
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Binary(sid, Bytes::from_static(b"Hello"))
    );

    dbg!("a");
    // And echoes them back through the read half, in order.
    match client.next().await {
        Some(Ok(EioEvent::Message(msg))) => assert_eq!(msg, "Hello"),
        other => panic!("expected echoed message, got {other:?}"),
    }
    dbg!("a");
    match client.next().await {
        Some(Ok(EioEvent::Binary(data))) => assert_eq!(data, Bytes::from_static(b"Hello")),
        other => panic!("expected echoed binary, got {other:?}"),
    }
    dbg!("a");
}
