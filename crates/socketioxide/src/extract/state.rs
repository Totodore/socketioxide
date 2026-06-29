use std::sync::Arc;

use crate::adapter::Adapter;
use crate::handler::{FromConnectParts, FromDisconnectParts, FromMessageParts};
use crate::socket::{DisconnectReason, Socket};
use socketioxide_core::Value;

/// An Extractor that contains a [`Clone`] of a state previously set with [`SocketIoBuilder::with_state`](crate::io::SocketIoBuilder).
/// It implements [`std::ops::Deref`] to access the inner type so you can use it as a normal reference.
///
/// The specified state type must be the same as the one set with [`SocketIoBuilder::with_state`](crate::io::SocketIoBuilder).
/// If it is not the case, the handler won't be called and an error log will be print if the `tracing` feature is enabled.
///
/// The state is shared between the entire socket.io app context.
///
/// ### Example
/// ```
/// # use socketioxide::{SocketIo, extract::{SocketRef, State}};
/// # use serde::{Serialize, Deserialize};
/// # use std::sync::{Arc, atomic::{Ordering, AtomicUsize}};
/// #[derive(Default, Clone)]
/// struct MyAppData {
///     user_cnt: Arc<AtomicUsize>,
/// }
/// impl MyAppData {
///     fn add_user(&self) {
///         self.user_cnt.fetch_add(1, Ordering::SeqCst);
///     }
///     fn rm_user(&self) {
///         self.user_cnt.fetch_sub(1, Ordering::SeqCst);
///     }
/// }
/// let (_, io) = SocketIo::builder().with_state(MyAppData::default()).build_svc();
/// io.ns("/", async |socket: SocketRef, state: State<MyAppData>| {
///     state.add_user();
///     println!("User count: {}", state.user_cnt.load(Ordering::SeqCst));
/// });
/// ```
///
/// ### Accessing the state from a custom extractor
///
/// Extractors are composable: rather than reaching into the socket internals,
/// a custom extractor can simply call another extractor — such as [`State`] —
/// inside its own implementation. Because [`State`] derefs to the inner value,
/// no manual `get_state` plumbing is required.
///
/// ```
/// # use socketioxide::handler::{FromConnectParts, Value};
/// # use socketioxide::extract::State;
/// # use socketioxide::adapter::Adapter;
/// # use socketioxide::socket::Socket;
/// # use socketioxide::SocketIo;
/// # use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
/// /// A custom extractor that yields the index of the current connection,
/// /// derived from a counter held in the shared application state.
/// struct ConnIndex(usize);
///
/// impl<A: Adapter> FromConnectParts<A> for ConnIndex {
///     type Error = std::convert::Infallible;
///     fn from_connect_parts(s: &Arc<Socket<A>>, auth: &Option<Value>) -> Result<Self, Self::Error> {
///         // Reuse the `State` extractor instead of calling `s.get_io().get_state()` directly.
///         let idx = State::<Arc<AtomicUsize>>::from_connect_parts(s, auth)
///             .map(|cnt| cnt.fetch_add(1, Ordering::SeqCst))
///             .unwrap_or(0);
///         Ok(ConnIndex(idx))
///     }
/// }
///
/// let (_, io) = SocketIo::builder()
///     .with_state(Arc::new(AtomicUsize::new(0)))
///     .build_svc();
/// io.ns("/", async |idx: ConnIndex| println!("connection #{}", idx.0));
/// ```
pub struct State<T>(pub T);

/// It was impossible to find the given state and therefore the handler won't be called.
pub struct StateNotFound<T>(std::marker::PhantomData<T>);

impl<T> std::fmt::Display for StateNotFound<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "State of type {} not found, maybe you forgot to insert it in the state map?",
            std::any::type_name::<T>()
        )
    }
}
impl<T> std::fmt::Debug for StateNotFound<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StateNotFound {}", std::any::type_name::<T>())
    }
}
impl<T> std::error::Error for StateNotFound<T> {}

impl<A: Adapter, T: Clone + Send + Sync + 'static> FromConnectParts<A> for State<T> {
    type Error = StateNotFound<T>;
    fn from_connect_parts(s: &Arc<Socket<A>>, _: &Option<Value>) -> Result<Self, StateNotFound<T>> {
        s.get_io()
            .get_state::<T>()
            .map(State)
            .ok_or(StateNotFound(std::marker::PhantomData))
    }
}
impl<A: Adapter, T: Clone + Send + Sync + 'static> FromDisconnectParts<A> for State<T> {
    type Error = StateNotFound<T>;
    fn from_disconnect_parts(
        s: &Arc<Socket<A>>,
        _: DisconnectReason,
    ) -> Result<Self, StateNotFound<T>> {
        s.get_io()
            .get_state::<T>()
            .map(State)
            .ok_or(StateNotFound(std::marker::PhantomData))
    }
}
impl<A: Adapter, T: Clone + Send + Sync + 'static> FromMessageParts<A> for State<T> {
    type Error = StateNotFound<T>;
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        _: &mut Value,
        _: &Option<i64>,
    ) -> Result<Self, StateNotFound<T>> {
        s.get_io()
            .get_state::<T>()
            .map(State)
            .ok_or(StateNotFound(std::marker::PhantomData))
    }
}

super::__impl_deref!(State);
