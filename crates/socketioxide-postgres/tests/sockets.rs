//! Sockets tests shared by the remote adapters.
mod fixture;

#[tokio::test]
async fn fetch_sockets() {
    adapter_test_suite::sockets::fetch_sockets::<fixture::Fixture>().await;
}

#[tokio::test]
async fn fetch_sockets_with_rooms() {
    adapter_test_suite::sockets::fetch_sockets_with_rooms::<fixture::Fixture>().await;
}

#[tokio::test]
async fn fetch_sockets_timeout() {
    adapter_test_suite::sockets::fetch_sockets_timeout::<fixture::Fixture>().await;
}

#[tokio::test]
async fn remote_socket_emit() {
    adapter_test_suite::sockets::remote_socket_emit::<fixture::Fixture>().await;
}

#[tokio::test]
async fn remote_socket_emit_with_ack() {
    adapter_test_suite::sockets::remote_socket_emit_with_ack::<fixture::Fixture>().await;
}
