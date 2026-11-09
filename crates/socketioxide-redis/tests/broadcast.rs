//! Broadcast tests shared by the remote adapters.
mod fixture;

#[tokio::test]
async fn broadcast() {
    adapter_test_suite::broadcast::broadcast::<fixture::Fixture>().await;
}

#[tokio::test]
async fn broadcast_rooms() {
    adapter_test_suite::broadcast::broadcast_rooms::<fixture::Fixture>().await;
}

#[tokio::test]
async fn broadcast_with_ack() {
    adapter_test_suite::broadcast::broadcast_with_ack::<fixture::Fixture>().await;
}

#[tokio::test]
async fn broadcast_with_ack_timeout() {
    adapter_test_suite::broadcast::broadcast_with_ack_timeout::<fixture::Fixture>().await;
}

#[tokio::test]
async fn emit_with_ack_timeout_applies_to_remote_sockets() {
    adapter_test_suite::broadcast::emit_with_ack_timeout_applies_to_remote_sockets::<
        fixture::Fixture,
    >()
    .await;
}
