use std::sync::Arc;

use socketioxide::{adapter::LocalAdapter, Socket};
use tracing::info;

pub async fn handler(socket: Arc<Socket<LocalAdapter>>) {
    info!("Socket connected on / with id: {}", socket.sid);

    socket.on("message", |socket, message: String, _, _| async move {
        info!("Message received: {}", message);
    });

    socket.on("nickname", |socket, nickname: String, _, _| async move {
        info!("Nickname received: {}", nickname);
    });

}
