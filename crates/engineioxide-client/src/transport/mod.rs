use engineioxide_core::TransportType;
use tokio_tungstenite::WebSocketStream;

use crate::HttpClient;

pub mod polling;

pub enum Transport<S> {
    Polling(HttpClient<S>),
    Websocket(),
}

impl<S> Transport<S> {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling(_) => TransportType::Polling,
            Transport::Websocket() => TransportType::Websocket,
        }
    }
}
