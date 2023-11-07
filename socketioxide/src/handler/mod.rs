//! Functions and types used to handle incoming connections and messages.
//! There is two main types of handlers: [`ConnectHandler`] and [`MessageHandler`].
//! Both handlers can be async or not.
mod connect;
pub mod extract;
mod message;

pub use connect::FromConnectParts;
pub(crate) use connect::{BoxedConnectHandler, ConnectHandler};
pub(crate) use message::{BoxedMessageHandler, MessageHandler};
pub use message::{FromMessage, FromMessageParts};
/// A struct used to erase the type of a [`ConnectHandler`] or [`MessageHandler`] so it can be stored in a map
pub(crate) struct MakeErasedHandler<H, A, T> {
    handler: H,
    adapter: std::marker::PhantomData<A>,
    type_: std::marker::PhantomData<T>,
}
impl<H, A, T> MakeErasedHandler<H, A, T> {
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            adapter: std::marker::PhantomData,
            type_: std::marker::PhantomData,
        }
    }
}
