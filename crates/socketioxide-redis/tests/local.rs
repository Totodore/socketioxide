//! Check that each adapter function with a broadcast options that is [`Local`] returns an immediate future
mod fixture;

macro_rules! assert_now {
    ($fut:expr) => {
        #[allow(unused_must_use)]
        futures_util::FutureExt::now_or_never($fut)
            .expect("Returned future should be sync")
            .unwrap()
    };
}

#[tokio::test]
async fn test_local_fns() {
    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", || ()).await.unwrap();
    io2.ns("/", || ()).await.unwrap();

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;

    timeout_rcv!(&mut rx1); // connect packet
    timeout_rcv!(&mut rx2); // connect packet

    assert_now!(io1.local().emit("test", "test"));
    assert_now!(io1.local().emit_with_ack::<_, ()>("test", "test"));
    assert_now!(io1.local().join("test"));
    assert_now!(io1.local().leave("test"));
    assert_now!(io1.local().disconnect());
    assert_now!(io1.local().fetch_sockets());
}
