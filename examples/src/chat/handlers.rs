use std::sync::Arc;

use serde::Deserialize;
use socketioxide::{adapter::LocalAdapter, Socket};
use tracing::info;

#[derive(Deserialize, Clone, Debug)]
struct Nickname(String);

#[derive(Deserialize)]
struct Auth {
    pub nickname: Nickname,
}

pub async fn handler(socket: Arc<Socket<LocalAdapter>>) {
    info!("Socket connected on / with id: {}", socket.sid);
    if let Ok(data) = socket.handshake.data::<Auth>() {
        info!("Nickname: {:?}", data.nickname);
        socket.extensions.insert(data.nickname);
    } else {
        info!("No nickname provided, disconnecting...");
        socket.disconnect().ok();
        return;
    }

    socket.on("message", |socket, message: String, _, _| async move {
        let Nickname(ref nickname) = *socket.extensions.get().unwrap();
        info!("Message received from {}: {}", nickname, message);
    });

    socket.on("nickname", |socket, nickname: Nickname, _, _| async move {
        let previous = socket.extensions.insert(nickname.clone());
        info!("Nickname changed from {:?} to {:?}", previous, nickname);
    });
}
