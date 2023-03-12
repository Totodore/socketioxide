use std::vec;

use futures::{SinkExt, StreamExt};
use http::Request;
use hyper::upgrade::Upgraded;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{protocol::Role, Message};

use crate::{packet::Packet, futures::ResponseFuture};

pub fn upgrade_ws_connection<T, F>(req: Request<T>) -> ResponseFuture<F>
where
    T: Send + 'static,
{
	let headers = req.headers().clone();
    tokio::task::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let ws = WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                println!("upgrade success: {:?}", ws.get_config());
                handle_ws_connection(ws);
            }
            Err(e) => println!("upgrade error: {}", e),
        }
    });
	ResponseFuture::upgrade_response(headers)
}

fn handle_ws_connection(ws_stream: WebSocketStream<Upgraded>) {
    println!("WebSocket connection established");

    let (mut tx, mut rx) = ws_stream.split();

    tokio::spawn(async move {
        loop {
            if let Some(msg) = rx.next().await {
                if let Ok(msg) = msg {
                    println!("Received message: {:?}", msg);
                    match msg {
                        Message::Text(msg) => {
                            let packet: Packet = msg.try_into().unwrap();
                            println!("Packet: {:?}", packet);
                            handle_packet(packet).await;
                        }
                        Message::Binary(_) => todo!(),
                        Message::Ping(_) => tx.send(Message::Pong(vec![])).await.unwrap(),
                        Message::Pong(_) => todo!(),
                        Message::Close(_) => println!("Close message received"),
                        Message::Frame(_) => assert!(false),
                    };
                    let msg: String = Packet::Pong.try_into().unwrap();
                    tx.send(Message::text(msg + &"probe".to_string()))
                        .await
                        .unwrap();
                } else {
                    println!("Error: {:?}", msg.err());
                    break;
                }
            } else {
                println!("Connection closed");
                break;
            }
        }
    });
}

async fn handle_packet(packet: Packet) {
    match packet {
        Packet::Open(_) => todo!(),
        Packet::Close => todo!(),
        Packet::Ping => todo!(),
        Packet::Pong => todo!(),
        Packet::Message(_) => todo!(),
        Packet::Upgrade => todo!(),
        Packet::Noop => todo!(),
    }
}
