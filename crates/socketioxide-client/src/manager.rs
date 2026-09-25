//! engine io connections manager, used to multiplex many socket.io
//! sockets onto a single low level connection.
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use engineioxide_client::{
    Client, EioEvent, EngineIoClientConfig,
    errors::ConnectError,
    flavors::{
        TransportSvc,
        testing::{EngineSvc, TestingFlavor},
    },
};
use futures_util::{Sink, Stream};
use http::uri;
use socketioxide_core::{
    Sid, Value,
    packet::Packet,
    parser::{Parse, ParseConfig, ParserState},
};
use tokio::{sync::mpsc, task};

use crate::{
    parser::{ParseError, Parser},
    socket::{DisconnectReason, Socket},
};

static IO_MANAGER: std::sync::OnceLock<IoManager> = std::sync::OnceLock::new();

type ConnId = Sid;

#[derive(Default)]
pub struct IoManager {
    conns: Arc<RwLock<HashMap<Option<uri::Authority>, Conn>>>,
}

impl IoManager {
    fn new() -> Self {
        IoManager::default()
    }
}

struct Conn {
    dispatcher: task::AbortHandle,
    dispatcher_emitter: mpsc::Sender<SocketEntry>,

    engine_id: Sid,
}

impl IoManager {
    fn get() -> &'static Self {
        IO_MANAGER.get_or_init(IoManager::new)
    }
}

impl Conn {
    fn new<S: TransportSvc>(client: Client<S>) -> Self {
        let engine_id = client.sid();

        let (dispatcher_emitter, rx) = mpsc::channel(5);
        let dispatcher = tokio::spawn(dispatcher(rx, client)).abort_handle();

        Self {
            dispatcher,
            dispatcher_emitter,

            engine_id,
        }
    }

    async fn create_socket(&self) -> Arc<Socket> {
        let (tx, queue) = mpsc::channel(255); //TODO: dyn config
        let socket = Arc::new(Socket::new(tx));

        let entry = SocketEntry {
            socket: socket.clone(),
            queue,
        };
        self.dispatcher_emitter.send(entry).await.ok();

        socket
    }
}

/// Instanciate a new socket
pub async fn io(svc: impl EngineSvc) -> Arc<Socket> {
    let manager = IoManager::get();
    let config = EngineIoClientConfig::default();
    let authority = config.uri.authority().cloned();

    //TODO: check if mutex is better.
    if let Some(conn) = manager.conns.write().unwrap().get_mut(&authority) {
        conn.create_socket().await
    } else {
        let client = eio_connect(svc).await.unwrap();

        let conn = Conn::new(client);
        let socket = conn.create_socket().await;

        manager.conns.write().unwrap().insert(authority, conn);

        socket
    }
}

async fn eio_connect<Svc: EngineSvc>(svc: Svc) -> Result<Client<TestingFlavor<Svc>>, ConnectError> {
    let config = EngineIoClientConfig::builder().uri("/socket.io").build()?;
    Client::connect_with_testbed(svc, config).await
}

/// A socket of the connection, with the queue of the packets it emits.
struct SocketEntry {
    socket: Arc<Socket>,
    queue: mpsc::Receiver<Packet>,
}

/// The dispatcher task of an engine.io connection.
///
/// Sockets created on this connection are registered through `reg` and are
/// notified once it is over.
async fn dispatcher<S: TransportSvc>(mut reg: mpsc::Receiver<SocketEntry>, client: Client<S>) {
    let mut dispatcher = Dispatcher::new();
    // the engine.io client is polled from both directions, it has to stay pinned.
    let mut client = std::pin::pin!(client);

    let reason = std::future::poll_fn(|cx| dispatcher.poll(cx, client.as_mut(), &mut reg)).await;

    for entry in &dispatcher.sockets {
        entry.socket.close(reason);
    }
}

/// Drives both directions of an engine.io connection.
///
/// Incoming payloads are decoded into socket.io packets and routed to the socket
/// of their namespace, and the packets emitted by those sockets are encoded and
/// written back.
///
/// Both directions are driven from a single poll pass on purpose: the engine.io
/// client runs its heartbeat and its transport flushes while its stream is
/// polled, so awaiting a write (a polling `POST` completes with its response)
/// would stall the reads and time the connection out.
struct Dispatcher {
    sockets: Vec<SocketEntry>,
    /// The events of the packet being written, its binary attachments included.
    pending: VecDeque<EioEvent>,
    /// Round robin cursor, so a busy socket cannot starve the other ones.
    cursor: usize,
    /// `false` once no new socket can be registered on this connection.
    reg_open: bool,

    parser: Parser,
    config: ParseConfig,
    /// Holds the partial binary packets until their attachments are received.
    state: ParserState,
}

impl Dispatcher {
    fn new() -> Self {
        Self {
            sockets: Vec::new(),
            pending: VecDeque::new(),
            cursor: 0,
            reg_open: true,

            parser: Parser::default(),
            config: ParseConfig::default(),
            state: ParserState::default(),
        }
    }

    /// Poll both directions of the connection, ready with the reason it is over.
    fn poll<S: TransportSvc>(
        &mut self,
        cx: &mut Context<'_>,
        client: Pin<&mut Client<S>>,
        reg: &mut mpsc::Receiver<SocketEntry>,
    ) -> Poll<DisconnectReason> {
        match self.poll_inner(cx, client, reg) {
            Ok(()) => Poll::Pending,
            Err(reason) => Poll::Ready(reason),
        }
    }

    fn poll_inner<S: TransportSvc>(
        &mut self,
        cx: &mut Context<'_>,
        mut client: Pin<&mut Client<S>>,
        reg: &mut mpsc::Receiver<SocketEntry>,
    ) -> Result<(), DisconnectReason> {
        self.poll_register(cx, reg);

        // one read and one write per iteration, so neither direction starves the other.
        loop {
            let read = self.read_one(cx, client.as_mut())?;
            let wrote = self.write_one(cx, client.as_mut())?;
            if !read && !wrote {
                break;
            }
        }

        // send out what was written in this pass. An incomplete flush is resumed
        // on the next one, it registered its waker.
        if let Poll::Ready(Err(_err)) = client.poll_flush(cx) {
            #[cfg(feature = "tracing")]
            tracing::debug!(?_err, "engine.io flush error");
            return Err(DisconnectReason::TransportError);
        }

        // nothing can be sent or received anymore once every socket is gone for good.
        if !self.reg_open && self.sockets.is_empty() {
            return Err(DisconnectReason::TransportClose);
        }

        Ok(())
    }

    /// Register the sockets created since the last pass.
    fn poll_register(&mut self, cx: &mut Context<'_>, reg: &mut mpsc::Receiver<SocketEntry>) {
        while self.reg_open {
            match reg.poll_recv(cx) {
                Poll::Ready(Some(entry)) => self.sockets.push(entry),
                Poll::Ready(None) => self.reg_open = false,
                Poll::Pending => break,
            }
        }
    }

    /// Read one payload from the connection and dispatch it to its namespace.
    /// Returns whether anything was read.
    fn read_one<S: TransportSvc>(
        &mut self,
        cx: &mut Context<'_>,
        client: Pin<&mut Client<S>>,
    ) -> Result<bool, DisconnectReason> {
        let event = match client.poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => event,
            Poll::Ready(Some(Err(_err))) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_err, "engine.io stream error");
                return Err(DisconnectReason::TransportError);
            }
            Poll::Ready(None) => return Err(DisconnectReason::TransportClose),
            Poll::Pending => return Ok(false),
        };

        let packet = match event {
            EioEvent::Message(data) => self.parser.decode_str(&self.config, &self.state, data),
            EioEvent::Binary(data) => self.parser.decode_bin(&self.config, &self.state, data),
            EioEvent::Disconnect => return Err(DisconnectReason::TransportClose),
            // engine.io level events, there is nothing to dispatch.
            EioEvent::Connect(_) | EioEvent::Upgrade(_) => return Ok(true),
        };

        match packet {
            Ok(packet) => match self.sockets.iter().find(|e| e.socket.ns == packet.ns) {
                Some(entry) => entry.socket.recv(packet.inner),
                None => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(ns = ?packet.ns, "no socket connected to this namespace");
                }
            },
            // the binary attachments of this packet are still to be received.
            Err(ParseError::NeedsMoreBinaryData) => (),
            Err(_err) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_err, "failed to decode an incoming packet");
            }
        }

        Ok(true)
    }

    /// Write one payload of the socket packets to the connection.
    /// Returns whether anything was written.
    fn write_one<S: TransportSvc>(
        &mut self,
        cx: &mut Context<'_>,
        mut client: Pin<&mut Client<S>>,
    ) -> Result<bool, DisconnectReason> {
        if self.pending.is_empty() {
            let Poll::Ready(packet) = self.poll_next_packet(cx) else {
                return Ok(false);
            };
            self.encode(packet);
        }

        match client.as_mut().poll_ready(cx) {
            Poll::Ready(Ok(())) => (),
            Poll::Ready(Err(_err)) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_err, "engine.io sink error");
                return Err(DisconnectReason::TransportError);
            }
            // the packet stays queued until the transport can take it.
            Poll::Pending => return Ok(false),
        }

        // the sink grants its readiness for a single payload: the binary
        // attachments are written by the next passes.
        let event = self.pending.pop_front().expect("a packet was just encoded");
        if let Err(_err) = client.start_send(event) {
            #[cfg(feature = "tracing")]
            tracing::debug!(?_err, "engine.io sink error");
            return Err(DisconnectReason::TransportError);
        }

        Ok(true)
    }

    /// Take the next packet to write from the socket queues, in round robin.
    fn poll_next_packet(&mut self, cx: &mut Context<'_>) -> Poll<Packet> {
        let mut polled = 0;
        while polled < self.sockets.len() {
            let idx = (self.cursor + polled) % self.sockets.len();
            match self.sockets[idx].queue.poll_recv(cx) {
                Poll::Ready(Some(packet)) => {
                    self.cursor = idx + 1;
                    return Poll::Ready(packet);
                }
                // the socket is gone for good: stop dispatching to and from it.
                // Its queue would be ready with `None` forever otherwise.
                Poll::Ready(None) => {
                    self.sockets.swap_remove(idx);
                    self.cursor = 0;
                    polled = 0;
                }
                Poll::Pending => polled += 1,
            }
        }

        Poll::Pending
    }

    /// Encode a packet into the events to write: the payload itself followed by
    /// its binary attachments.
    fn encode(&mut self, packet: Packet) {
        match self.parser.encode(packet) {
            Value::Str(data, bins) => {
                self.pending.push_back(EioEvent::Message(data));
                self.pending
                    .extend(bins.into_iter().flatten().map(EioEvent::Binary));
            }
            Value::Bytes(data) => self.pending.push_back(EioEvent::Binary(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use super::*;

    /// A socket entry whose queue is fed through the returned sender.
    fn entry() -> (mpsc::Sender<Packet>, SocketEntry) {
        let (tx, queue) = mpsc::channel(4);
        // the socket own sender is not the one feeding the queue here, so that
        // dropping `tx` closes it.
        let (socket_tx, _) = mpsc::channel(1);
        let entry = SocketEntry {
            socket: Arc::new(Socket::new(socket_tx)),
            queue,
        };
        (tx, entry)
    }

    fn packet(event: &'static str) -> Packet {
        Packet::event("/", Value::Str(event.into(), None))
    }

    /// The cursor rotates over the sockets, so a socket that always has a packet
    /// ready cannot starve the other ones.
    #[test]
    fn poll_next_packet_round_robins_over_the_sockets() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut dispatcher = Dispatcher::new();

        let (tx0, entry0) = entry();
        let (tx1, entry1) = entry();
        dispatcher.sockets.extend([entry0, entry1]);

        tx0.try_send(packet("a")).unwrap();
        tx0.try_send(packet("c")).unwrap();
        tx1.try_send(packet("b")).unwrap();

        assert_eq!(
            dispatcher.poll_next_packet(&mut cx),
            Poll::Ready(packet("a"))
        );
        assert_eq!(
            dispatcher.poll_next_packet(&mut cx),
            Poll::Ready(packet("b"))
        );
        assert_eq!(
            dispatcher.poll_next_packet(&mut cx),
            Poll::Ready(packet("c"))
        );
        assert_eq!(dispatcher.poll_next_packet(&mut cx), Poll::Pending);
    }

    /// A socket that is gone is removed, its queue would be ready forever.
    #[test]
    fn poll_next_packet_drops_the_gone_sockets() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut dispatcher = Dispatcher::new();

        let (tx0, entry0) = entry();
        let (tx1, entry1) = entry();
        dispatcher.sockets.extend([entry0, entry1]);

        tx1.try_send(packet("a")).unwrap();
        drop(tx0);

        assert_eq!(
            dispatcher.poll_next_packet(&mut cx),
            Poll::Ready(packet("a"))
        );
        assert_eq!(dispatcher.sockets.len(), 1);

        drop(tx1);
        assert_eq!(dispatcher.poll_next_packet(&mut cx), Poll::Pending);
        assert!(dispatcher.sockets.is_empty());
    }
}
