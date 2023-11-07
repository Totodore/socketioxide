//! Functions and types used to handle incoming connections and messages.
//! There is two main types of handlers: [`ConnectHandler`] and [`MessageHandler`].
//! Both handlers can be async or not.
mod connect;
pub mod extract;
mod message;

use std::sync::Arc;

pub(crate) use connect::{BoxedConnectHandler, ConnectHandler};
pub use message::{AckResponse, AckSender};
pub(crate) use message::{BoxedMessageHandler, MessageHandler};

use serde_json::Value;

use crate::{adapter::Adapter, socket::Socket};

/// A struct used to erase the type of a [`ConnectHandler`] or [`MessageHandler`] so it can be stored in a map
pub(crate) struct MakeErasedHandler<H, A, T, Fut> {
    handler: H,
    adapter: std::marker::PhantomData<A>,
    type_: std::marker::PhantomData<T>,
    fut: std::marker::PhantomData<Fut>,
}
impl<H, A, T, Fut> MakeErasedHandler<H, A, T, Fut> {
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            adapter: std::marker::PhantomData,
            type_: std::marker::PhantomData,
            fut: std::marker::PhantomData,
        }
    }
}

trait FromMessageParts<T, A: Adapter> {
    fn from_message_parts(s: &Arc<Socket<A>>, v: &Value, ack_id: Option<i64>);
}
