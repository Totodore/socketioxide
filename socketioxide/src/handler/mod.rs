//! Functions and types used to handle incoming connections and messages.
//! There is three main types of handlers: [connect], [message] and [disconnect].
//! All handlers can be async or not.
pub mod connect;
pub mod disconnect;
pub mod message;

pub(crate) use connect::BoxedConnectHandler;
pub use connect::{ConnectHandler, ConnectMiddleware, FromConnectParts};
pub(crate) use disconnect::BoxedDisconnectHandler;
pub use disconnect::{DisconnectHandler, FromDisconnectParts};
pub(crate) use message::BoxedMessageHandler;
pub use message::{FromMessage, FromMessageParts, MessageHandler};
/// A struct used to erase the type of a [`ConnectHandler`] or [`MessageHandler`] so it can be stored in a map
pub(crate) struct MakeErasedHandler<H, T> {
    handler: H,
    type_: std::marker::PhantomData<T>,
}
impl<H, T> MakeErasedHandler<H, T> {
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            type_: std::marker::PhantomData,
        }
    }
}
impl<H: Clone, T> Clone for MakeErasedHandler<H, T> {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            type_: std::marker::PhantomData,
        }
    }
}
