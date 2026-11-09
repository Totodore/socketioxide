//! Check that each adapter function with a broadcast options that is `Local`
//! returns an immediate future.
mod fixture;

#[tokio::test]
async fn test_local_fns() {
    adapter_test_suite::local::test_local_fns::<fixture::Fixture>().await;
}
