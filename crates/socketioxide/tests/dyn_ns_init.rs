//! The adapter of a dynamic namespace is initialized when the first client connects to it,
//! and this can take a while with a remote adapter. Clients connecting to this namespace
//! in the meantime must end up in the same namespace, with a single adapter initialization.

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use socketioxide::{SocketIo, adapter::Emitter, extract::SocketRef, socket::DisconnectReason};
use socketioxide_core::adapter::{
    BroadcastOptions, CoreAdapter, CoreLocalAdapter, DefinedAdapter, SocketEmitter, Spawnable,
};
use socketioxide_core::packet::Packet;
use tokio::sync::{Semaphore, mpsc};

const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
struct State {
    /// The number of `init` calls.
    inits: Arc<AtomicUsize>,
    /// Each `init` call consumes a permit to finish: a closed semaphore makes it fail.
    gate: Arc<Semaphore>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            inits: Arc::default(),
            gate: Arc::new(Semaphore::new(0)),
        }
    }
}

struct SlowAdapter {
    state: State,
    local: CoreLocalAdapter<Emitter>,
}

struct InitRes(Pin<Box<dyn Future<Output = ()> + Send>>);
impl Spawnable for InitRes {
    fn spawn(self) {
        tokio::spawn(self.0);
    }
}

impl DefinedAdapter for SlowAdapter {}
impl CoreAdapter<Emitter> for SlowAdapter {
    type Error = std::convert::Infallible;
    type State = State;
    type AckStream = <Emitter as SocketEmitter>::AckStream;
    type InitRes = InitRes;

    fn new(state: &Self::State, local: CoreLocalAdapter<Emitter>) -> Self {
        Self {
            state: state.clone(),
            local,
        }
    }

    fn init(self: Arc<Self>, on_success: impl FnOnce() + Send + 'static) -> Self::InitRes {
        self.state.inits.fetch_add(1, Ordering::SeqCst);
        InitRes(Box::pin(async move {
            if let Ok(permit) = self.state.gate.acquire().await {
                permit.forget();
                on_success();
            }
        }))
    }

    async fn broadcast_with_ack(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
        timeout: Option<Duration>,
    ) -> Result<Self::AckStream, Self::Error> {
        Ok(self.local.broadcast_with_ack(packet, opts, timeout).0)
    }

    fn get_local(&self) -> &CoreLocalAdapter<Emitter> {
        &self.local
    }
}

type Connected = mpsc::UnboundedReceiver<SocketRef<SlowAdapter>>;

fn create_server(state: State) -> (SocketIo<SlowAdapter>, Connected) {
    let (_svc, io) = SocketIo::builder()
        .with_adapter::<SlowAdapter>(state)
        .build_svc();
    let (tx, rx) = mpsc::unbounded_channel();
    io.dyn_ns("/dyn/{id}", async move |s: SocketRef<SlowAdapter>| {
        tx.send(s).unwrap();
    })
    .unwrap();
    (io, rx)
}

async fn next_socket(rx: &mut Connected) -> SocketRef<SlowAdapter> {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("the socket should be connected")
        .unwrap()
}

#[tokio::test]
async fn concurrent_connections_share_the_same_namespace() {
    let state = State::default();
    let (io, mut rx) = create_server(state.clone());

    let (_tx1, mut srx1) = io.new_dummy_sock("/dyn/1", ()).await;
    let (_tx2, mut srx2) = io.new_dummy_sock("/dyn/1", ()).await;
    let (_tx3, mut srx3) = io.new_dummy_sock("/dyn/1", ()).await;
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);

    state.gate.add_permits(10);

    for _ in 0..3 {
        next_socket(&mut rx).await;
    }
    for srx in [&mut srx1, &mut srx2, &mut srx3] {
        let packet = tokio::time::timeout(TIMEOUT, srx.recv()).await.unwrap();
        assert!(packet.is_some());
    }

    // Every socket must be reachable from the namespace.
    let ns = io.of("/dyn/1").expect("the namespace should exist");
    assert_eq!(ns.sockets().len(), 3);
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);
}

/// A socket of an overwritten namespace never got its disconnect handler called.
#[tokio::test]
async fn disconnect_handler_is_called_for_each_concurrent_connection() {
    let state = State::default();
    let (disconnect_tx, mut disconnect_rx) = mpsc::unbounded_channel();
    let (_svc, io) = SocketIo::builder()
        .with_adapter::<SlowAdapter>(state.clone())
        .build_svc();
    io.dyn_ns("/dyn/{id}", async move |s: SocketRef<SlowAdapter>| {
        let disconnect_tx = disconnect_tx.clone();
        s.on_disconnect(async move |reason: DisconnectReason| {
            disconnect_tx.send(reason).unwrap();
        });
    })
    .unwrap();

    let (tx1, _srx1) = io.new_dummy_sock("/dyn/1", ()).await;
    let (tx2, _srx2) = io.new_dummy_sock("/dyn/1", ()).await;
    state.gate.add_permits(10);
    tokio::time::sleep(Duration::from_millis(20)).await;

    tx1.try_send(engineioxide::Packet::Close).unwrap();
    tx2.try_send(engineioxide::Packet::Close).unwrap();

    for _ in 0..2 {
        tokio::time::timeout(TIMEOUT, disconnect_rx.recv())
            .await
            .expect("the disconnect handler should be called")
            .unwrap();
    }
}

#[tokio::test]
async fn different_namespaces_are_initialized_independently() {
    let state = State::default();
    let (io, mut rx) = create_server(state.clone());

    let (_tx1, _srx1) = io.new_dummy_sock("/dyn/1", ()).await;
    let (_tx2, _srx2) = io.new_dummy_sock("/dyn/2", ()).await;
    assert_eq!(state.inits.load(Ordering::SeqCst), 2);

    state.gate.add_permits(10);
    next_socket(&mut rx).await;
    next_socket(&mut rx).await;
    assert_eq!(io.of("/dyn/1").unwrap().sockets().len(), 1);
    assert_eq!(io.of("/dyn/2").unwrap().sockets().len(), 1);
}

#[tokio::test]
async fn connecting_after_the_init_does_not_init_again() {
    let state = State::default();
    state.gate.add_permits(10);
    let (io, mut rx) = create_server(state.clone());

    let (_tx1, _srx1) = io.new_dummy_sock("/dyn/1", ()).await;
    next_socket(&mut rx).await;
    let (_tx2, _srx2) = io.new_dummy_sock("/dyn/1", ()).await;
    next_socket(&mut rx).await;

    assert_eq!(state.inits.load(Ordering::SeqCst), 1);
    assert_eq!(io.of("/dyn/1").unwrap().sockets().len(), 2);
}

/// A failed initialization must not block the next attempts.
#[tokio::test]
async fn connection_after_a_failed_init_tries_again() {
    let state = State::default();
    let failing = Arc::new(Semaphore::new(0));
    failing.close();
    let (io, mut rx) = create_server(State {
        inits: state.inits.clone(),
        gate: failing,
    });

    let (_tx1, _srx1) = io.new_dummy_sock("/dyn/1", ()).await;
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);
    assert!(io.of("/dyn/1").is_none());

    let (_tx2, _srx2) = io.new_dummy_sock("/dyn/1", ()).await;
    assert_eq!(state.inits.load(Ordering::SeqCst), 2);
    assert!(rx.try_recv().is_err());
}
