//! [`MessageHandler`] trait and implementations, used to handle the message events
//! It has a flexible axum-like API, you can put any arguments as long as it implements the [`FromMessageParts`] trait or the [`FromMessage`] trait for the last argument
//! Handlers can be async or not
//!
//! ## Example
//! ```rust
//! # use socketioxide::SocketIo;
//! # use serde_json::Error;
//! # use socketioxide::extract::*;
//! let (svc, io) = SocketIo::new_svc();
//! io.ns("/", |s: SocketRef| {
//!     s.on("/event", |s: SocketRef, Data::<String>(data)| {
//!        println!("Socket received event with data: {}", data);
//!     });
//!     s.on("/event_with_ack", |s: SocketRef, ack: AckSender| {
//!       ack.send("ack data").ok();
//!     });
//!
//!     // `Bin` extractor must be the last argument because it consumes the rest of the packet
//!     s.on("/binary_event", |s: SocketRef, TryData::<String>(data), Bin(bin)| {
//!       println!("Socket received event with data: {} and binary data: {:?}", data, bin);
//!     })
//! });
//! ```
use std::sync::Arc;

use futures::Future;
use serde_json::Value;

use crate::adapter::Adapter;
use crate::socket::Socket;

use super::MakeErasedHandler;

/// A Type Erased [`MessageHandler`] so it can be stored in a HashMap
pub(crate) type BoxedMessageHandler<A> = Box<dyn ErasedMessageHandler<A>>;

pub(crate) trait ErasedMessageHandler<A: Adapter>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>);
}

/// Define a handler for the connect event
/// It is implemented for closures with up to 16 arguments that implement the [`FromMessageParts`] trait or the [`FromMessage`] trait for the last argument
/// The closure can be async or not
#[cfg_attr(
    nightly_error_messages,
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor. \nSee `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
    )
)]
pub trait MessageHandler<A: Adapter, T>: Send + Sync + 'static {
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>);
    fn phantom(&self) -> std::marker::PhantomData<T> {
        std::marker::PhantomData
    }
}

impl<A, T, H> MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: MessageHandler<A, T>,
    A: Adapter,
{
    pub fn new_message_boxed(inner: H) -> Box<dyn ErasedMessageHandler<A>> {
        Box::new(MakeErasedHandler::new(inner))
    }
}

impl<A, T, H> ErasedMessageHandler<A> for MakeErasedHandler<H, A, T>
where
    T: Send + Sync + 'static,
    H: MessageHandler<A, T>,
    A: Adapter,
{
    #[inline(always)]
    fn call(&self, s: Arc<Socket<A>>, v: Value, p: Vec<Vec<u8>>, ack_id: Option<i64>) {
        self.handler.call(s, v, p, ack_id);
    }
}

mod private {
    #[derive(Debug, Clone, Copy)]
    pub enum ViaParts {}

    #[derive(Debug, Clone, Copy)]
    pub enum ViaRequest {}

    #[derive(Debug, Clone, Copy)]
    pub enum Sync {}
    #[derive(Debug, Clone, Copy)]
    pub enum Async {}
}

/// A trait used to extract the arguments from the message event
/// The `Result` is used to return an error if the extraction fails, in this case the handler is not called
#[cfg_attr(
    nightly_error_messages,
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor. \nSee `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
    )
)]
pub trait FromMessageParts<A: Adapter>: Sized {
    fn from_message_parts(
        s: &Arc<Socket<A>>,
        v: &mut Value,
        p: &mut Vec<Vec<u8>>,
        ack_id: &Option<i64>,
    ) -> Result<Self, ()>;
}

/// A trait used to extract and consume the arguments from the message event
/// The `Result` is used to return an error if the extraction fails, in this case the handler is not called
#[cfg_attr(
    nightly_error_messages,
    diagnostic::on_unimplemented(
        note = "Function argument is not a valid socketio extractor. \nSee `https://docs.rs/socketioxide/latest/socketioxide/extract/index.html` for details",
    )
)]
pub trait FromMessage<A: Adapter, M = private::ViaRequest>: Sized {
    fn from_message(
        s: Arc<Socket<A>>,
        v: Value,
        p: Vec<Vec<u8>>,
        ack_id: Option<i64>,
    ) -> Result<Self, ()>;
}

/// All the types that implement [`FromMessageParts`] also implement [`FromMessage`]
impl<A, T> FromMessage<A, private::ViaParts> for T
where
    T: FromMessageParts<A>,
    A: Adapter,
{
    fn from_message(
        s: Arc<Socket<A>>,
        mut v: Value,
        mut p: Vec<Vec<u8>>,
        ack_id: Option<i64>,
    ) -> Result<Self, ()> {
        Self::from_message_parts(&s, &mut v, &mut p, &ack_id)
    }
}

/// Empty Async handler
impl<A, F, Fut> MessageHandler<A, (private::Async,)> for F
where
    F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    A: Adapter,
{
    fn call(&self, _: Arc<Socket<A>>, _: Value, _: Vec<Vec<u8>>, _: Option<i64>) {
        let fut = (self.clone())();
        tokio::spawn(fut);
    }
}

/// Empty Sync handler
impl<A, F> MessageHandler<A, (private::Sync,)> for F
where
    F: FnOnce() + Send + Sync + Clone + 'static,
    A: Adapter,
{
    fn call(&self, _: Arc<Socket<A>>, _: Value, _: Vec<Vec<u8>>, _: Option<i64>) {
        (self.clone())();
    }
}

macro_rules! impl_async_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, M, $($ty,)* $last, Fut> MessageHandler<A, (private::Async, M, $($ty,)* $last,)> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Send + Sync + Clone + 'static,
            Fut: Future<Output = ()> + Send + 'static,
            A: Adapter,
            $( $ty: FromMessageParts<A> + Send, )*
            $last: FromMessage<A, M> + Send,
        {
            fn call(&self, s: Arc<Socket<A>>, mut v: Value, mut p: Vec<Vec<u8>>, ack_id: Option<i64>) {
                $(
                    let $ty = match $ty::from_message_parts(&s, &mut v, &mut p, &ack_id) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                )*
                let last = match $last::from_message(s, v, p, ack_id) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                (self.clone())($($ty,)* last);
            }
        }
    };
}
macro_rules! impl_handler {
    (
        [$($ty:ident),*], $last:ident
    ) => {
        #[allow(non_snake_case, unused)]
        impl<A, F, M, $($ty,)* $last> MessageHandler<A, (private::Sync, M, $($ty,)* $last,)> for F
        where
            F: FnOnce($($ty,)* $last,) + Send + Sync + Clone + 'static,
            A: Adapter,
            $( $ty: FromMessageParts<A> + Send, )*
            $last: FromMessage<A, M> + Send,
        {
            fn call(&self, s: Arc<Socket<A>>, mut v: Value, mut p: Vec<Vec<u8>>, ack_id: Option<i64>) {
                $(
                    let $ty = match $ty::from_message_parts(&s, &mut v, &mut p, &ack_id) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                )*
                let last = match $last::from_message(s, v, p, ack_id) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                (self.clone())($($ty,)* last);
            }
        }
    };
}

#[rustfmt::skip]
macro_rules! all_the_tuples {
    ($name:ident) => {
        $name!([], T1);
        $name!([T1], T2);
        $name!([T1, T2], T3);
        $name!([T1, T2, T3], T4);
        $name!([T1, T2, T3, T4], T5);
        $name!([T1, T2, T3, T4, T5], T6);
        $name!([T1, T2, T3, T4, T5, T6], T7);
        $name!([T1, T2, T3, T4, T5, T6, T7], T8);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13], T14);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14], T15);
        $name!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15], T16);
    };
}

all_the_tuples!(impl_handler);
all_the_tuples!(impl_async_handler);
