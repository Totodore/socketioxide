//! Tests of the `permessage-deflate` extension against a real server,
//! with a raw client as `tungstenite` isn't able to read compressed frames.
#![cfg(feature = "ws-deflate")]

use std::{io::Cursor, sync::Arc};

use bytes::{Buf, Bytes, BytesMut};
use engineioxide::{
    Str,
    config::{EngineIoConfig, PerMessageDeflateConfig},
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::tungstenite::protocol::frame::{
    FrameHeader,
    coding::{Data, OpCode},
};

const TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// Echoes all the messages it receives.
#[derive(Debug)]
struct EchoHandler;
impl EngineIoHandler for EchoHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, _: Arc<Socket<()>>) {}
    fn on_disconnect(&self, _: Arc<Socket<()>>, _: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<()>>) {
        socket.emit(msg).unwrap();
    }
    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<()>>) {
        socket.emit_binary(data).unwrap();
    }
}

async fn start_server(config: EngineIoConfig) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let svc = EngineIoService::with_config(Arc::new(EchoHandler), config);

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let svc = svc.clone();
            tokio::spawn(
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .with_upgrades(),
            );
        }
    });
    port
}

fn enabled() -> EngineIoConfig {
    EngineIoConfig::builder()
        .ws_per_message_deflate(PerMessageDeflateConfig::default().threshold(16))
        .build()
}

struct Frame {
    rsv1: bool,
    opcode: OpCode,
    payload: Vec<u8>,
}

struct Client {
    stream: TcpStream,
    buf: BytesMut,
    compress: Compress,
    decompress: Decompress,
    /// The `Sec-WebSocket-Extensions` response header
    extensions: Option<String>,
}

impl Client {
    async fn connect(port: u16, offer: Option<&str>, query: &str) -> Self {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let offer = offer
            .map(|offer| format!("Sec-WebSocket-Extensions: {offer}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "GET /engine.io/?EIO=4&transport=websocket{query} HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             Connection: Upgrade\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             {offer}\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();

        let mut buf = BytesMut::with_capacity(4096);
        let head_end = loop {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
            assert_ne!(stream.read_buf(&mut buf).await.unwrap(), 0);
        };
        let head = String::from_utf8(buf.split_to(head_end + 4).to_vec()).unwrap();
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        let extensions = head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("sec-websocket-extensions")
                .then(|| value.trim().to_owned())
        });

        Self {
            stream,
            buf,
            compress: Compress::new(Compression::default(), false),
            decompress: Decompress::new(false),
            extensions,
        }
    }

    async fn read_frame(&mut self) -> Frame {
        loop {
            let mut cursor = Cursor::new(&self.buf[..]);
            if let Some((header, len)) = FrameHeader::parse(&mut cursor).unwrap() {
                let header_len = cursor.position() as usize;
                let len = len as usize;
                if self.buf.len() >= header_len + len {
                    self.buf.advance(header_len);
                    assert!(header.is_final, "the server doesn't fragment");
                    assert!(header.mask.is_none(), "the server doesn't mask");
                    return Frame {
                        rsv1: header.rsv1,
                        opcode: header.opcode,
                        payload: self.buf.split_to(len).to_vec(),
                    };
                }
            }
            assert_ne!(self.stream.read_buf(&mut self.buf).await.unwrap(), 0);
        }
    }

    /// Reads the next data frame, skipping the heartbeat, and decompress it if needed.
    async fn read_message(&mut self) -> (OpCode, bool, Vec<u8>) {
        loop {
            let frame = self.read_frame().await;
            if frame.opcode == OpCode::Data(Data::Text) && frame.payload == b"2" {
                continue;
            }
            let payload = if frame.rsv1 {
                self.inflate(&frame.payload)
            } else {
                frame.payload
            };
            return (frame.opcode, frame.rsv1, payload);
        }
    }

    fn inflate(&mut self, payload: &[u8]) -> Vec<u8> {
        let input = [payload, &TAIL].concat();
        let mut out = Vec::with_capacity(1 << 20);
        let start = self.decompress.total_in() as usize;
        while self.decompress.total_in() as usize - start < input.len() {
            let consumed = self.decompress.total_in() as usize - start;
            self.decompress
                .decompress_vec(&input[consumed..], &mut out, FlushDecompress::None)
                .unwrap();
            out.reserve(1024);
        }
        out
    }

    async fn send(&mut self, opcode: OpCode, compress: bool, data: &[u8]) {
        let payload = if compress {
            let mut out = Vec::with_capacity(data.len() + 1024);
            self.compress
                .compress_vec(data, &mut out, FlushCompress::Sync)
                .unwrap();
            assert!(out.ends_with(&TAIL));
            out.truncate(out.len() - TAIL.len());
            out
        } else {
            data.to_vec()
        };

        let mask = [1, 2, 3, 4];
        let header = FrameHeader {
            rsv1: compress,
            opcode,
            mask: Some(mask),
            ..Default::default()
        };
        let mut bytes = Vec::new();
        header.format(payload.len() as u64, &mut bytes).unwrap();
        bytes.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, k)| b ^ k));
        self.stream.write_all(&bytes).await.unwrap();
    }

    /// Reads the open packet and returns the session id.
    async fn read_open(&mut self) -> String {
        let (opcode, rsv1, payload) = self.read_message().await;
        assert_eq!(opcode, OpCode::Data(Data::Text));
        assert!(!rsv1, "the open packet is under the threshold");
        let payload = String::from_utf8(payload).unwrap();
        assert!(payload.starts_with("0{"), "{payload}");
        let open: serde_json::Value = serde_json::from_str(&payload[1..]).unwrap();
        open["sid"].as_str().unwrap().to_owned()
    }
}

const TEXT: OpCode = OpCode::Data(Data::Text);
const BINARY: OpCode = OpCode::Data(Data::Binary);

#[tokio::test]
async fn offer_is_ignored_when_disabled() {
    let port = start_server(EngineIoConfig::default()).await;
    let mut client = Client::connect(port, Some("permessage-deflate"), "").await;
    assert_eq!(client.extensions, None);

    client.read_open().await;
    let msg = format!("4{}", "a".repeat(1024));
    client.send(TEXT, false, msg.as_bytes()).await;
    assert_eq!(client.read_message().await, (TEXT, false, msg.into_bytes()));
}

#[tokio::test]
async fn no_offer_means_no_compression() {
    let port = start_server(enabled()).await;
    let mut client = Client::connect(port, None, "").await;
    assert_eq!(client.extensions, None);

    client.read_open().await;
    let msg = format!("4{}", "a".repeat(1024));
    client.send(TEXT, false, msg.as_bytes()).await;
    assert_eq!(client.read_message().await, (TEXT, false, msg.into_bytes()));
}

#[tokio::test]
async fn compressed_frames_are_rejected_when_not_negotiated() {
    let port = start_server(enabled()).await;
    let mut client = Client::connect(port, None, "").await;
    client.read_open().await;
    client.send(TEXT, true, b"4hello").await;

    // The connection is closed by the server.
    let mut buf = [0; 1024];
    while client.stream.read(&mut buf).await.is_ok_and(|n| n > 0) {}
}

#[tokio::test]
async fn compressed_echo() {
    let port = start_server(enabled()).await;
    let offer = "permessage-deflate; client_max_window_bits";
    let mut client = Client::connect(port, Some(offer), "").await;
    assert_eq!(client.extensions.as_deref(), Some("permessage-deflate"));
    client.read_open().await;

    let text = format!("4{}", "hello world ".repeat(100));
    let binary = vec![42u8; 4096];
    // Two times to use the context from both sides.
    for _ in 0..2 {
        client.send(TEXT, true, text.as_bytes()).await;
        assert_eq!(
            client.read_message().await,
            (TEXT, true, text.clone().into_bytes())
        );

        client.send(BINARY, true, &binary).await;
        assert_eq!(client.read_message().await, (BINARY, true, binary.clone()));
    }

    // Under the threshold, and in a mix of compressed and plain messages.
    client.send(TEXT, true, b"4small").await;
    assert_eq!(
        client.read_message().await,
        (TEXT, false, b"4small".to_vec())
    );
    client.send(TEXT, false, text.as_bytes()).await;
    assert_eq!(
        client.read_message().await,
        (TEXT, true, text.clone().into_bytes())
    );
}

#[tokio::test]
async fn no_context_takeover() {
    let config = EngineIoConfig::builder()
        .ws_per_message_deflate(
            PerMessageDeflateConfig::default()
                .threshold(16)
                .server_no_context_takeover(true)
                .client_no_context_takeover(true),
        )
        .build();
    let port = start_server(config).await;
    let mut client = Client::connect(port, Some("permessage-deflate"), "").await;
    assert_eq!(
        client.extensions.as_deref(),
        Some("permessage-deflate; server_no_context_takeover; client_no_context_takeover")
    );
    client.read_open().await;

    let text = format!("4{}", "hello world ".repeat(100));
    for _ in 0..3 {
        // Both sides of the client have to forget about their context.
        client.compress.reset();
        client.decompress.reset(false);
        client.send(TEXT, true, text.as_bytes()).await;
        assert_eq!(
            client.read_message().await,
            (TEXT, true, text.clone().into_bytes())
        );
    }
}

#[tokio::test]
async fn upgrade_from_polling() {
    let port = start_server(enabled()).await;

    // Open a session with a polling request.
    let mut polling = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    polling
        .write_all(
            b"GET /engine.io/?EIO=4&transport=polling HTTP/1.1\r\n\
              Host: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut body = String::new();
    polling.read_to_string(&mut body).await.unwrap();
    let json = &body[body.find('{').unwrap()..=body.rfind('}').unwrap()];
    let open: serde_json::Value = serde_json::from_str(json).unwrap();
    let sid = open["sid"].as_str().unwrap();

    let query = format!("&sid={sid}");
    let mut client = Client::connect(port, Some("permessage-deflate"), &query).await;
    assert_eq!(client.extensions.as_deref(), Some("permessage-deflate"));

    client.send(TEXT, true, b"2probe").await;
    assert_eq!(
        client.read_message().await,
        (TEXT, false, b"3probe".to_vec())
    );
    client.send(TEXT, true, b"5").await;

    let text = format!("4{}", "hello world ".repeat(100));
    client.send(TEXT, true, text.as_bytes()).await;
    loop {
        let message = client.read_message().await;
        // The polling transport may leave a noop packet behind.
        if message.2 == b"6" {
            continue;
        }
        assert_eq!(message, (TEXT, true, text.into_bytes()));
        break;
    }
}
