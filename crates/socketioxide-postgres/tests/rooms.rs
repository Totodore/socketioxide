//! Rooms tests shared by the remote adapters.
mod fixture;

#[tokio::test]
async fn all_rooms() {
    adapter_test_suite::rooms::all_rooms::<fixture::Fixture>().await;
}

#[tokio::test]
async fn all_rooms_timeout() {
    adapter_test_suite::rooms::all_rooms_timeout::<fixture::Fixture>().await;
}

#[tokio::test]
async fn add_sockets() {
    adapter_test_suite::rooms::add_sockets::<fixture::Fixture>().await;
}

#[tokio::test]
async fn del_sockets() {
    adapter_test_suite::rooms::del_sockets::<fixture::Fixture>().await;
}
