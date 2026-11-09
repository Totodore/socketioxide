//! Shared test suite for the socketioxide remote adapters.
//!
//! Each adapter crate keeps its own fixture (the stub driver) and implements
//! [`Fixture`]. The tests themselves live here so they only have to be written
//! and fixed once.

use std::{future::Future, time::Duration};

use socketioxide::{SocketIo, adapter::Adapter, handler::ConnectHandler};
use socketioxide_core::adapter::DefinedAdapter;

/// Provides the servers used by the adapter tests.
pub trait Fixture {
    /// The adapter under test.
    type Adapter: Adapter + DefinedAdapter;

    /// Spawn `N` servers connected to each other.
    fn spawn_servers<const N: usize>() -> [SocketIo<Self::Adapter>; N];

    /// Spawn `N` servers with a fake server count, to trigger timeouts when
    /// expecting responses.
    fn spawn_buggy_servers<const N: usize>(timeout: Duration) -> [SocketIo<Self::Adapter>; N];

    /// Register a handler on the default namespace and wait for the adapter to
    /// be ready.
    fn add_ns<C, T>(io: &SocketIo<Self::Adapter>, handler: C) -> impl Future<Output = ()>
    where
        C: ConnectHandler<Self::Adapter, T>,
        T: Send + Sync + 'static;
}

#[macro_export]
macro_rules! timeout_rcv_err {
    ($srx:expr) => {
        tokio::time::timeout(std::time::Duration::from_millis(10), $srx.recv())
            .await
            .unwrap_err();
    };
}

#[macro_export]
macro_rules! timeout_rcv {
    ($srx:expr) => {
        TryInto::<String>::try_into(
            tokio::time::timeout(std::time::Duration::from_millis(10), $srx.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    ($srx:expr, $t:expr) => {
        TryInto::<String>::try_into(
            tokio::time::timeout(std::time::Duration::from_millis($t), $srx.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
}

/// Check that a future returns immediately, for operations that are local.
macro_rules! assert_now {
    ($fut:expr) => {
        #[allow(unused_must_use)]
        futures_util::FutureExt::now_or_never($fut)
            .expect("Returned future should be sync")
            .unwrap()
    };
}

pub mod broadcast;
pub mod local;
pub mod rooms;
pub mod sockets;
