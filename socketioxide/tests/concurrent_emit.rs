//! Test for concurrent emit (issue https://github.com/Totodore/socketioxide/issues/232)
//! Binary messages are splitted into one string packet and adjacent binary packets.
//!
//! Under high load, if the atomicity of the emit is not guaranteed, binary packets may be out of order.

use futures::StreamExt;
use socketioxide::extract::SocketRef;
use tokio_tungstenite::tungstenite::Message;

mod fixture;
mod utils;
use fixture::create_server;

use crate::fixture::create_ws_connection;

#[tokio::test]
pub async fn emit() {
    const PORT: u16 = 1502;
    use Message::*;
    let io = create_server(PORT).await;

    io.ns("/", move |socket: SocketRef| async move {
        for _ in 0..100 {
            let s = socket.clone();
            tokio::task::spawn_blocking(move || {
                for _ in 0..100 {
                    s.bin(vec![vec![1, 2, 3], vec![4, 5, 6]])
                        .emit("test", "bin")
                        .unwrap();
                }
            });
        }
    });

    // Spawn 5 clients and make them echo the ack
    let (mut _stx, mut srx) = create_ws_connection(PORT).await.split();
    assert_ok!(srx.next().await.unwrap());
    assert_ok!(srx.next().await.unwrap());

    let mut count = 0;
    const MSG: &str =
        r#"452-["test","bin",{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]"#;
    while let Some(msg) = srx.next().await {
        match assert_ok!(msg) {
            Text(msg) if count == 0 && msg == MSG => {
                assert_eq!(msg, MSG);
                count = (count + 1) % 3;
            }
            Binary(bin) if count == 1 => {
                assert_eq!(bin, vec![1, 2, 3]);
                count = (count + 1) % 3;
            }
            Binary(bin) if count == 2 => {
                assert_eq!(bin, vec![4, 5, 6]);
                count = (count + 1) % 3;
            }
            Ping(_) | Pong(_) | Text(_) | Close(_) => (),
            msg => panic!("unexpected message: {:?}, count: {}", msg, count),
        };
    }
}
