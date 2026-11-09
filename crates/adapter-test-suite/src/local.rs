//! Check that each adapter function with a broadcast options that is `Local`
//! returns an immediate future.

use super::Fixture;

pub async fn test_local_fns<F: Fixture>() {
    let [io1, io2] = F::spawn_servers::<2>();

    F::add_ns(&io1, async || ()).await;
    F::add_ns(&io2, async || ()).await;

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
