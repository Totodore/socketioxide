use engineio_server::layer::EngineIoHandler;
use engineio_server::socket::Socket as EIoSocket;
use engineio_server::errors::Error;

#[derive(Debug, Clone)]
pub struct Handler {

}


impl Handler {
    pub fn new() -> Self {
        Self {}
    }
}

#[engineio_server::async_trait]
impl EngineIoHandler for Handler {
    fn on_connect(&self, socket: &EIoSocket<Self>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &EIoSocket<Self>) {
        println!("socket disconnect {}", socket.sid);
    }

    async fn on_message(&self, msg: String, socket: &EIoSocket<Self>) -> Result<(), Error> {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).await  // This will wait until the buffer is drained
    }

    async fn on_binary(&self, data: Vec<u8>, socket: &EIoSocket<Self>) -> Result<(), Error> {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).await  // This will wait until the buffer is drained
    }
}