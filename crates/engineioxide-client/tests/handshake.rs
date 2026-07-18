use std::time::Duration;

use engineioxide_client::{
    EngineIoClientConfig,
    transport::{PollingTransport, WsTransport},
};

use crate::fixture::{Event, service};

mod fixture;

#[tokio::test]
async fn handshake_polling() {
    let (svc, mut rx) = service();
    let (_transport, open) = tokio::time::timeout(
        Duration::from_secs(1),
        PollingTransport::connect(svc, &EngineIoClientConfig::default()),
    )
    .await
    .expect("timeout while initializing polling transport conn")
    .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout while receiving server connect event");

    assert_eq!(event, Some(Event::Connect(open.sid)));
}

#[tokio::test]
async fn handshake_websocket() {
    let (svc, mut rx) = service();
    let (_transport, open) = tokio::time::timeout(
        Duration::from_secs(1),
        WsTransport::connect(svc, &EngineIoClientConfig::default()),
    )
    .await
    .expect("timeout while initializing polling transport conn")
    .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout while receiving server connect event");

    assert_eq!(event, Some(Event::Connect(open.sid)));
}
