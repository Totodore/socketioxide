use std::{str::FromStr, time::Duration};

use socketioxide::{
    adapter::Adapter, extract::SocketRef, operators::BroadcastOperators, socket::RemoteSocket,
    SocketIo,
};
use socketioxide_core::{adapter::RemoteSocketData, Sid, Str};
use tokio::time::Instant;

mod fixture;
fn extract_sid(data: &str) -> Sid {
    let data = data
        .split("\"sid\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap();
    Sid::from_str(data).unwrap()
}
async fn fetch_sockets_data<A: Adapter>(op: BroadcastOperators<A>) -> Vec<RemoteSocketData> {
    let mut sockets = op
        .fetch_sockets()
        .await
        .unwrap()
        .into_iter()
        .map(RemoteSocket::into_data)
        .collect::<Vec<_>>();
    sockets.sort_by(|a, b| a.id.cmp(&b.id));
    sockets
}
fn create_expected_sockets<const N: usize, A: Adapter>(
    ids: [Sid; N],
    ios: [&SocketIo<A>; N],
) -> [RemoteSocketData; N] {
    let mut i = 0;
    let mut sockets = ios.map(|io| {
        let id = ids[i];
        i += 1;
        RemoteSocketData {
            id,
            server_id: io.config().server_id,
            ns: Str::from("/"),
        }
    });
    sockets.sort_by(|a, b| a.id.cmp(&b.id));
    sockets
}

#[tokio::test]
pub async fn fetch_sockets() {
    let [io1, io2, io3] = fixture::spawn_servers::<3>();

    io1.ns("/", || ());
    io2.ns("/", || ());
    io3.ns("/", || ());

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;
    let (_, mut rx3) = io3.new_dummy_sock("/", ()).await;

    let id1 = extract_sid(&timeout_rcv!(&mut rx1));
    let id2 = extract_sid(&timeout_rcv!(&mut rx2));
    let id3 = extract_sid(&timeout_rcv!(&mut rx3));

    let mut expected_sockets = create_expected_sockets([id1, id2, id3], [&io1, &io2, &io3]);
    expected_sockets.sort_by(|a, b| a.id.cmp(&b.id));

    let sockets = fetch_sockets_data(io1.broadcast()).await;
    assert_eq!(sockets, expected_sockets);

    let sockets = fetch_sockets_data(io2.broadcast()).await;
    assert_eq!(sockets, expected_sockets);

    let sockets = fetch_sockets_data(io3.broadcast()).await;
    assert_eq!(sockets, expected_sockets);
}

#[tokio::test]
pub async fn fetch_sockets_with_rooms() {
    let [io1, io2, io3] = fixture::spawn_servers::<3>();
    let handler = |rooms: &'static [&'static str]| move |socket: SocketRef<_>| socket.join(rooms);

    io1.ns("/", handler(&["room1", "room2"]));
    io2.ns("/", handler(&["room2", "room3"]));
    io3.ns("/", handler(&["room3", "room1"]));

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;
    let (_, mut rx3) = io3.new_dummy_sock("/", ()).await;

    let id1 = extract_sid(&timeout_rcv!(&mut rx1));
    let id2 = extract_sid(&timeout_rcv!(&mut rx2));
    let id3 = extract_sid(&timeout_rcv!(&mut rx3));

    let sockets = fetch_sockets_data(io1.to("room1")).await;
    assert_eq!(sockets, create_expected_sockets([id1, id3], [&io1, &io3]));

    let sockets = fetch_sockets_data(io1.to("room2")).await;
    assert_eq!(sockets, create_expected_sockets([id1, id2], [&io1, &io2]));

    let sockets = fetch_sockets_data(io1.to("room3")).await;
    assert_eq!(sockets, create_expected_sockets([id2, id3], [&io2, &io3]));
}

#[tokio::test]
pub async fn fetch_sockets_timeout() {
    const TIMEOUT: Duration = Duration::from_millis(50);
    let [io1, io2] = fixture::spawn_buggy_servers(TIMEOUT);

    io1.ns("/", || ());
    io2.ns("/", || ());

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;

    timeout_rcv!(&mut rx1); // connect packet
    timeout_rcv!(&mut rx2); // connect packet

    let now = Instant::now();
    io1.fetch_sockets().await.unwrap();
    assert!(now.elapsed() >= TIMEOUT);
}

#[tokio::test]
pub async fn remote_socket_emit() {
    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", || ());
    io2.ns("/", || ());

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;

    timeout_rcv!(&mut rx1); // connect packet
    timeout_rcv!(&mut rx2); // connect packet

    let sockets = io1.fetch_sockets().await.unwrap();
    for socket in sockets {
        socket.emit("test", "hello").await.unwrap();
    }

    assert_eq!(timeout_rcv!(&mut rx1), r#"42["test","hello"]"#);
    assert_eq!(timeout_rcv!(&mut rx2), r#"42["test","hello"]"#);
}

#[tokio::test]
pub async fn remote_socket_emit_with_ack() {
    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", || ());
    io2.ns("/", || ());

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;

    timeout_rcv!(&mut rx1); // connect packet
    timeout_rcv!(&mut rx2); // connect packet

    let sockets = io1.fetch_sockets().await.unwrap();
    for socket in sockets {
        #[allow(unused_must_use)]
        socket
            .emit_with_ack::<_, ()>("test", "hello")
            .await
            .unwrap();
    }

    assert_eq!(timeout_rcv!(&mut rx1), r#"431["test","hello"]"#);
    assert_eq!(timeout_rcv!(&mut rx2), r#"431["test","hello"]"#);
}
