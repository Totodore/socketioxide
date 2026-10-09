//! engine io connections manager, used to multiplex many socket.io
//! sockets onto a single low level connection.
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    task::{Context, Poll},
    time::Duration,
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
    Str, Value,
    packet::{ConnectPacket, Packet, PacketData},
    parser::{Parse, ParseConfig, ParserState},
};
use tokio::{sync::mpsc, task, time};

use crate::{
    parser::{ParseError, Parser},
    socket::{DisconnectReason, Socket},
};

static IO_MANAGER: std::sync::OnceLock<IoManager> = std::sync::OnceLock::new();

/// The delay before a reconnection attempt. It doubles with every failed one, up
/// to [`RECONNECT_DELAY_MAX`], and is jittered by up to [`RECONNECT_JITTER`] of
/// itself. Same defaults as the reference implementation.
//TODO: user provided config
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(5);
const RECONNECT_JITTER: f64 = 0.5;

#[derive(Default, Clone)]
pub struct IoManager {
    //TODO: complex hashing to match all possibilities
    conns: Arc<RwLock<HashMap<Option<uri::Authority>, Conn>>>,
}

impl IoManager {
    pub fn new() -> Self {
        IoManager::default()
    }

    pub fn global() -> &'static Self {
        IO_MANAGER.get_or_init(IoManager::new)
    }

    pub async fn connect(&self, svc: impl EngineSvc) -> Arc<Socket> {
        let config = eio_config();
        let authority = config.uri.authority().cloned();
        let ns = Str::from("/");

        // multiplex onto the connection of this authority when there is one.
        //TODO: check if mutex is better.
        let socket = self
            .conns
            .read()
            .unwrap()
            .get(&authority)
            .and_then(|conn| conn.socket(&ns));
        if let Some(socket) = socket {
            return socket;
        }

        let client = eio_connect(svc.clone(), &config).await.unwrap();

        let mut conns = self.conns.write().unwrap();

        // Another call might have created the conn in the meantime,
        // if so drop the current connection attempt and return the one previously created.
        if let Some(socket) = conns.get(&authority).and_then(|conn| conn.socket(&ns)) {
            return socket;
        }

        let manager = self.clone();
        let (conn, socket) = Conn::new(svc, manager, config, client, authority.clone(), ns);
        conns.insert(authority, conn);

        socket
    }

    fn release_conn(&self, authority: &Option<uri::Authority>) -> Option<Conn> {
        self.conns.write().unwrap().remove(authority)
    }
}

//TODO: find a way to reduce complexity, make a full-schema of:
// * engine.io - low-level conn close conditions
// * socket.io disconnection conditions
// * reconnect behaviors
//
// * Try to check if it couldn't be composable as streams
/// An engine.io connection and the socket.io sockets multiplexed on it.
///
/// The sockets are owned here rather than by the dispatcher of a single
/// engine.io connection: they outlive it, keep buffering what they emit while it
/// is being reopened, and are connected to their namespace again on the new one.
struct Conn {
    sockets: Arc<Mutex<Vec<SocketEntry>>>,
    /// Wakes the supervisor up when a socket is registered.
    wake: mpsc::UnboundedSender<()>,
    supervisor: task::AbortHandle,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.supervisor.abort();
    }
}

impl Conn {
    /// Open a connection for the first socket of `ns`.
    fn new<Svc: EngineSvc>(
        svc: Svc,
        manager: IoManager,
        config: EngineIoClientConfig,
        client: Client<TestingFlavor<Svc>>,
        authority: Option<uri::Authority>,
        ns: Str,
    ) -> (Self, Arc<Socket>) {
        let (socket, entry) = create_socket(ns);
        let sockets = Arc::new(Mutex::new(vec![entry]));

        // unbounded: a socket is registered while the connection map is locked,
        // so it cannot await.
        let (wake, wake_rx) = mpsc::unbounded_channel();
        let supervisor = tokio::spawn(supervisor(
            svc,
            manager,
            config,
            client,
            sockets.clone(),
            wake_rx,
            authority,
        ))
        .abort_handle();

        let conn = Self {
            sockets,
            wake,
            supervisor,
        };

        (conn, socket)
    }

    /// The socket of `ns` on this connection, registering a new one when there is
    /// none: the reference implementation hands out one socket per namespace.
    ///
    /// `None` once its supervisor is gone, the connection cannot serve anything
    /// anymore and has to be replaced.
    fn socket(&self, ns: &Str) -> Option<Arc<Socket>> {
        if self.wake.is_closed() {
            return None;
        }

        let mut sockets = self.sockets.lock().unwrap();
        if let Some(entry) = sockets.iter().find(|entry| &entry.socket.ns == ns) {
            return Some(entry.socket.clone());
        }

        let (socket, entry) = create_socket(ns.clone());
        sockets.push(entry);
        drop(sockets);

        // the supervisor connects it to its namespace.
        self.wake.send(()).ok();

        Some(socket)
    }
}

fn create_socket(ns: Str) -> (Arc<Socket>, SocketEntry) {
    let (tx, queue) = mpsc::channel(255); //TODO: dyn config
    let socket = Arc::new(Socket::new(tx, ns));

    let entry = SocketEntry {
        socket: socket.clone(),
        queue,
        ns_state: NsState::New,
    };

    (socket, entry)
}

/// Instanciate a new socket
pub async fn io(svc: impl EngineSvc) -> Arc<Socket> {
    IoManager::global().connect(svc).await
}

fn eio_config() -> EngineIoClientConfig {
    //TODO: user provided config
    EngineIoClientConfig::builder()
        .uri("/socket.io")
        .build()
        .expect("the default uri should be valid")
}

async fn eio_connect<Svc: EngineSvc>(
    svc: Svc,
    config: &EngineIoClientConfig,
) -> Result<Client<TestingFlavor<Svc>>, ConnectError> {
    Client::connect_with_testbed(svc, config.clone()).await
}

/// The state of a socket on its namespace. A socket has to be accepted by the
/// server before anything else is sent for it, on every connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NsState {
    /// Its connect packet still has to be sent.
    New,
    /// Its connect packet was sent, the server has yet to accept it.
    Connecting,
    /// Accepted by the server: what it emits is dispatched.
    Connected,
}

/// A socket of the connection, with the queue of the packets it emits.
struct SocketEntry {
    socket: Arc<Socket>,
    queue: mpsc::Receiver<Packet>,
    ns_state: NsState,
}

/// Keeps the engine.io connection of its sockets up.
///
/// The connection is reopened, with a growing delay, until its last socket is
/// disconnected: the reference implementation reconnects the same way while a
/// socket is still active. What the sockets emit meanwhile is buffered in their
/// queue and flushed once the server accepted them again.
async fn supervisor<Svc: EngineSvc>(
    svc: Svc,
    manager: IoManager,
    config: EngineIoClientConfig,
    client: Client<TestingFlavor<Svc>>,
    sockets: Arc<Mutex<Vec<SocketEntry>>>,
    mut wake: mpsc::UnboundedReceiver<()>,
    authority: Option<uri::Authority>,
) {
    let mut client = Some(client);
    let mut backoff = Backoff::default();

    // the released connection is held until this task is over: dropping it
    // aborts the task.
    let _conn = loop {
        let client = match client.take() {
            Some(client) => client,
            None => match eio_connect(svc.clone(), &config).await {
                Ok(client) => {
                    backoff.reset();
                    client
                }
                Err(_err) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(?_err, "reconnection failed");
                    time::sleep(backoff.next_delay()).await;
                    continue;
                }
            },
        };

        let reason = run_dispatcher(client, &sockets, &mut wake).await;
        notify_closed_sockets(&sockets, reason);

        if sockets.lock().unwrap().is_empty()
            //TODO: it should return always Some, ensure the invariant
            && let Some(conn) = manager.release_conn(&authority)
        {
            break conn;
        }

        time::sleep(backoff.next_delay()).await;
    };
}

/// Notify the sockets that were connected that their connection is over. They
/// are connected again on the next one: a socket that is not connected anymore
/// was disconnected on purpose and has already been notified.
fn notify_closed_sockets(sockets: &Mutex<Vec<SocketEntry>>, reason: DisconnectReason) {
    // collected so that the handlers do not run while the list is locked.
    let connected: Vec<_> = sockets
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.ns_state == NsState::Connected)
        .map(|entry| entry.socket.clone())
        .collect();

    for socket in connected {
        socket.close(reason);
    }
}

/// Every socket has to be accepted on its namespace again by the server when a
/// new connection is opened for them.
fn reset_namespaces(sockets: &Mutex<Vec<SocketEntry>>) {
    for entry in sockets.lock().unwrap().iter_mut() {
        entry.ns_state = NsState::New;
    }
}

/// Run the dispatcher of one engine.io connection until it is over.
async fn run_dispatcher<S: TransportSvc>(
    client: Client<S>,
    sockets: &Mutex<Vec<SocketEntry>>,
    wake: &mut mpsc::UnboundedReceiver<()>,
) -> DisconnectReason {
    reset_namespaces(sockets);

    let client = std::pin::pin!(client);
    let mut dispatcher = Dispatcher::new(client, wake);

    std::future::poll_fn(|cx| dispatcher.poll(cx, sockets)).await
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
struct Dispatcher<'a, S: TransportSvc> {
    /// The events of the packet being written, its binary attachments included.
    pending: VecDeque<EioEvent>,
    /// Round robin cursor, so a busy socket cannot starve the other ones.
    cursor: usize,
    /// Set once the connection is being closed for good.
    closing: bool,

    parser: Parser,
    config: ParseConfig,
    /// Holds the partial binary packets until their attachments are received.
    state: ParserState,

    client: Pin<&'a mut Client<S>>,
    wake: &'a mut mpsc::UnboundedReceiver<()>,
}

impl<'a, S: TransportSvc> Dispatcher<'a, S> {
    fn new(client: Pin<&'a mut Client<S>>, wake: &'a mut mpsc::UnboundedReceiver<()>) -> Self {
        Self {
            pending: VecDeque::new(),
            cursor: 0,
            closing: false,

            parser: Parser::default(),
            config: ParseConfig::default(),
            state: ParserState::default(),

            client,
            wake,
        }
    }

    /// Poll both directions of the connection, ready with the reason it is over.
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        sockets: &Mutex<Vec<SocketEntry>>,
    ) -> Poll<DisconnectReason> {
        // a socket is registered in the list directly, the ping only wakes this
        // task up so that it picks it up.
        //
        // TODO: WTF, explain the concept
        while let Poll::Ready(Some(())) = self.wake.poll_recv(cx) {}

        let mut sockets = sockets.lock().unwrap();
        match self.poll_inner(cx, &mut sockets) {
            Ok(()) => Poll::Pending,
            Err(reason) => Poll::Ready(reason),
        }
    }

    /// Read an write from the engine.io stream in //.
    /// Returns a [`DisconnectReason`] in case of shutdown, or `Ok`
    /// if it should be polled again.
    fn poll_inner(
        &mut self,
        cx: &mut Context<'_>,
        sockets: &mut Vec<SocketEntry>,
    ) -> Result<(), DisconnectReason> {
        // one read and one write per iteration, so neither direction starves the other.
        loop {
            let read = self.read_one(cx, sockets)?;
            let wrote = self.write_one(cx, sockets)?;
            if !read && !wrote {
                break;
            }
        }

        // send out what was written in this pass. An incomplete flush is resumed
        // on the next one, it registered its waker.
        if let Poll::Ready(Err(_err)) = self.client.as_mut().poll_flush(cx) {
            #[cfg(feature = "tracing")]
            tracing::debug!(?_err, "engine.io flush error");
            return Err(DisconnectReason::TransportError);
        }

        // The connection is closed once its last socket left its namespace, as
        // the reference implementation does when no socket is active anymore.
        // A socket registered in the meantime is served by a new connection.
        if sockets.is_empty() {
            self.closing = true;
        }

        // the disconnect packets are handed to the transport before the close
        // packet, they are flushed together.
        if self.closing && self.pending.is_empty() {
            match self.client.as_mut().poll_close(cx) {
                Poll::Ready(Ok(())) => return Err(DisconnectReason::IoClientDisconnect),
                Poll::Ready(Err(_err)) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(?_err, "engine.io close error");
                    return Err(DisconnectReason::TransportError);
                }
                Poll::Pending => (),
            }
        }

        Ok(())
    }

    /// Read one payload from the connection and dispatch it to its namespace.
    /// Returns whether anything was read.
    fn read_one(
        &mut self,
        cx: &mut Context<'_>,
        sockets: &mut Vec<SocketEntry>,
    ) -> Result<bool, DisconnectReason> {
        let event = match self.client.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => event,
            Poll::Ready(Some(Err(_err))) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_err, "engine.io stream error");
                return Err(DisconnectReason::TransportError);
            }
            // the engine.io client fuses its stream as soon as it closes.
            Poll::Ready(None) if self.closing => return Ok(false),
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

        let packet = match packet {
            Ok(packet) => packet,
            // the binary attachments of this packet are still to be received.
            Err(ParseError::NeedsMoreBinaryData) => return Ok(true),
            Err(_err) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(?_err, "failed to decode an incoming packet");
                return Ok(true);
            }
        };

        let Some(idx) = sockets
            .iter()
            .position(|entry| entry.socket.ns == packet.ns)
        else {
            #[cfg(feature = "tracing")]
            tracing::debug!(ns = ?packet.ns, "no socket connected to this namespace");
            return Ok(true);
        };

        match packet.inner {
            // the server accepted the socket on its namespace: it is given an id
            // and everything it buffered meanwhile is dispatched right after.
            PacketData::Connect(payload) => {
                match self
                    .parser
                    .decode_default::<ConnectPacket>(payload.as_ref())
                {
                    Ok(ConnectPacket { sid }) => {
                        sockets[idx].ns_state = NsState::Connected;
                        sockets[idx].socket.set_connected(sid);
                    }
                    Err(_err) => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(?_err, "failed to read the connect payload");
                    }
                }
            }
            // the namespace refused the socket, connecting it again would not help.
            PacketData::ConnectError(_reason) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(ns = ?packet.ns, "namespace connection refused: {_reason}");
                sockets[idx]
                    .socket
                    .close(DisconnectReason::IoServerDisconnect);
                self.deregister(sockets, idx);
            }
            // the socket handles the disconnection itself, it just cannot be
            // dispatched to anymore.
            PacketData::Disconnect => {
                sockets[idx].socket.recv(PacketData::Disconnect);
                self.deregister(sockets, idx);
            }
            packet => sockets[idx].socket.recv(packet),
        }

        Ok(true)
    }

    /// Write one payload of the socket packets to the connection.
    /// Returns whether anything was written.
    fn write_one(
        &mut self,
        cx: &mut Context<'_>,
        sockets: &mut Vec<SocketEntry>,
    ) -> Result<bool, DisconnectReason> {
        if self.pending.is_empty() {
            let Poll::Ready((idx, packet)) = self.poll_next_packet(cx, sockets) else {
                return Ok(false);
            };

            // a socket leaving its namespace sends its disconnect packet as its
            // last one: it is deregistered as soon as the packet is encoded.
            let disconnect = matches!(packet.inner, PacketData::Disconnect);
            self.encode(packet);
            if disconnect {
                sockets[idx]
                    .socket
                    .close(DisconnectReason::IoClientDisconnect);
                self.deregister(sockets, idx);
            }
        }

        match self.client.as_mut().poll_ready(cx) {
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
        if let Err(_err) = self.client.as_mut().start_send(event) {
            #[cfg(feature = "tracing")]
            tracing::debug!(?_err, "engine.io sink error");
            return Err(DisconnectReason::TransportError);
        }

        Ok(true)
    }

    /// Take the next packet to write, with the index of the socket it is for.
    ///
    /// A socket is connected to its namespace before anything else is sent for
    /// it: what it emits until the server accepts it waits in its queue, which
    /// is the buffer that survives a reconnection.
    fn poll_next_packet(
        &mut self,
        cx: &mut Context<'_>,
        sockets: &mut Vec<SocketEntry>,
    ) -> Poll<(usize, Packet)> {
        let new = sockets
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.ns_state == NsState::New);
        if let Some((idx, entry)) = new {
            entry.ns_state = NsState::Connecting;
            //TODO: auth payload
            return Poll::Ready((idx, Packet::connect(entry.socket.ns.clone(), None)));
        }

        let mut polled = 0;
        while polled < sockets.len() {
            let idx = (self.cursor + polled) % sockets.len();
            if sockets[idx].ns_state != NsState::Connected {
                polled += 1;
                continue;
            }

            match sockets[idx].queue.poll_recv(cx) {
                Poll::Ready(Some(packet)) => {
                    self.cursor = idx + 1;
                    return Poll::Ready((idx, packet));
                }
                // the socket was dropped without disconnecting: its queue would
                // be ready with `None` forever otherwise.
                Poll::Ready(None) => {
                    self.deregister(sockets, idx);
                    polled = 0;
                }
                Poll::Pending => polled += 1,
            }
        }

        Poll::Pending
    }

    /// Stop dispatching to and from a socket that left its namespace. Its queue
    /// is dropped, so anything it emits afterwards fails.
    fn deregister(&mut self, sockets: &mut Vec<SocketEntry>, idx: usize) {
        sockets.swap_remove(idx);
        self.cursor = 0;
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

/// The exponentially growing delay between two connection attempts.
#[derive(Default)]
struct Backoff {
    attempts: u32,
}

impl Backoff {
    fn next_delay(&mut self) -> Duration {
        let delay = RECONNECT_DELAY
            .saturating_mul(1u32 << self.attempts.min(16))
            .min(RECONNECT_DELAY_MAX);
        self.attempts = self.attempts.saturating_add(1);

        // jittered so that many clients that lost the same server do not come
        // back all at once.
        let jitter = delay.mul_f64(RECONNECT_JITTER * jitter_ratio());
        delay.saturating_add(jitter).min(RECONNECT_DELAY_MAX)
    }

    fn reset(&mut self) {
        self.attempts = 0;
    }
}

/// A ratio in `[0, 1)` taken from the clock: the jitter does not need a good
/// random, and `rand` is not a dependency of this crate.
fn jitter_ratio() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());

    f64::from(nanos % 1_000) / 1_000.0
}

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use socketioxide_core::Sid;

    use super::*;

    /// A socket entry of `ns` whose queue is fed through the returned sender.
    fn entry(ns: &'static str, ns_state: NsState) -> (mpsc::Sender<Packet>, SocketEntry) {
        let (tx, queue) = mpsc::channel(4);
        // the sender of the socket is not the one feeding the queue here, so that
        // dropping `tx` closes it.
        let (socket_tx, _) = mpsc::channel(1);
        let entry = SocketEntry {
            socket: Arc::new(Socket::new(socket_tx, ns.into())),
            queue,
            ns_state,
        };

        (tx, entry)
    }

    fn connected(count: usize) -> (Vec<mpsc::Sender<Packet>>, Vec<SocketEntry>) {
        (0..count).map(|_| entry("/", NsState::Connected)).unzip()
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
        let (txs, mut sockets) = connected(2);

        txs[0].try_send(packet("a")).unwrap();
        txs[0].try_send(packet("c")).unwrap();
        txs[1].try_send(packet("b")).unwrap();

        let mut next = || dispatcher.poll_next_packet(&mut cx, &mut sockets);
        assert_eq!(next(), Poll::Ready((0, packet("a"))));
        assert_eq!(next(), Poll::Ready((1, packet("b"))));
        assert_eq!(next(), Poll::Ready((0, packet("c"))));
        assert_eq!(next(), Poll::Pending);
    }

    /// A socket that is gone is removed, its queue would be ready forever.
    #[test]
    fn poll_next_packet_drops_the_gone_sockets() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut dispatcher = Dispatcher::new();
        let (mut txs, mut sockets) = connected(2);

        txs[1].try_send(packet("a")).unwrap();
        txs.remove(0);

        assert_eq!(
            dispatcher.poll_next_packet(&mut cx, &mut sockets),
            Poll::Ready((0, packet("a")))
        );
        assert_eq!(sockets.len(), 1);

        txs.clear();
        assert_eq!(
            dispatcher.poll_next_packet(&mut cx, &mut sockets),
            Poll::Pending
        );
        assert!(sockets.is_empty());
    }

    /// A socket is connected to its namespace before anything else is sent for
    /// it, and what it emits meanwhile is buffered until the server accepts it.
    /// This is what survives a reconnection.
    #[test]
    fn a_socket_is_connected_before_its_packets_are_sent() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut dispatcher = Dispatcher::new();
        let (tx, entry) = entry("/admin", NsState::New);
        let mut sockets = vec![entry];

        tx.try_send(packet("buffered")).unwrap();

        // the connect packet comes first, the queued one is held back.
        let connect = dispatcher.poll_next_packet(&mut cx, &mut sockets);
        assert_eq!(connect, Poll::Ready((0, Packet::connect("/admin", None))));
        assert_eq!(sockets[0].ns_state, NsState::Connecting);
        assert_eq!(
            dispatcher.poll_next_packet(&mut cx, &mut sockets),
            Poll::Pending
        );

        // once accepted, the buffer is flushed.
        sockets[0].ns_state = NsState::Connected;
        assert_eq!(
            dispatcher.poll_next_packet(&mut cx, &mut sockets),
            Poll::Ready((0, packet("buffered")))
        );
    }

    /// A socket asking to leave its namespace is deregistered, and its packet is
    /// still written so the server hears about it.
    #[test]
    fn a_disconnect_packet_deregisters_its_socket() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut dispatcher = Dispatcher::new();
        let (txs, mut sockets) = connected(2);

        txs[0].try_send(Packet::disconnect("/")).unwrap();

        let Poll::Ready((idx, packet)) = dispatcher.poll_next_packet(&mut cx, &mut sockets) else {
            panic!("the disconnect packet should be dispatched");
        };
        assert!(matches!(packet.inner, PacketData::Disconnect));

        dispatcher.encode(packet);
        dispatcher.deregister(&mut sockets, idx);

        assert_eq!(sockets.len(), 1);
        assert_eq!(dispatcher.pending.len(), 1);
    }

    /// The delay grows with the attempts, up to its ceiling.
    #[test]
    fn the_reconnection_delay_grows_up_to_its_ceiling() {
        let mut backoff = Backoff::default();

        let first = backoff.next_delay();
        let second = backoff.next_delay();
        assert!(first >= RECONNECT_DELAY && first <= RECONNECT_DELAY_MAX);
        assert!(second >= first);

        for _ in 0..10 {
            assert!(backoff.next_delay() <= RECONNECT_DELAY_MAX);
        }

        backoff.reset();
        assert!(backoff.next_delay() < RECONNECT_DELAY_MAX);
    }

    /// A new connection connects every socket to its namespace again, which is
    /// what makes them usable after a reconnection.
    #[test]
    fn a_new_connection_connects_every_socket_again() {
        let (_txs, sockets): (Vec<_>, Vec<_>) =
            [NsState::Connected, NsState::Connecting, NsState::New]
                .into_iter()
                .map(|state| entry("/", state))
                .unzip();
        let sockets = Mutex::new(sockets);

        reset_namespaces(&sockets);

        let sockets = sockets.lock().unwrap();
        assert!(sockets.iter().all(|entry| entry.ns_state == NsState::New));
    }

    /// A connection lost under a connected socket notifies it, and it loses the
    /// id that connection gave it. A socket that already left is left alone, it
    /// was notified when it did.
    #[test]
    fn a_lost_connection_notifies_the_connected_sockets() {
        let (_tx, connected) = entry("/", NsState::Connected);
        let (_tx2, left) = entry("/admin", NsState::Connecting);
        // both are given an id, only the connected one should lose it.
        connected.socket.set_connected(Sid::new());
        left.socket.set_connected(Sid::new());

        let sockets = Mutex::new(vec![connected, left]);
        notify_closed_sockets(&sockets, DisconnectReason::TransportClose);

        let sockets = sockets.lock().unwrap();
        assert!(sockets[0].socket.id().is_none());
        assert!(sockets[1].socket.id().is_some());
    }
}
