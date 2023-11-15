//! Functions and types used to handle incoming connections and messages.
//! There is two main types of handlers: [`ConnectHandler`] and [`MessageHandler`].
//! Both handlers can be async or not.
pub mod connect;
pub mod extract;
pub mod message;

pub(crate) use connect::BoxedConnectHandler;
pub use connect::{ConnectHandler, FromConnectParts};
pub(crate) use message::BoxedMessageHandler;
pub use message::{FromMessage, FromMessageParts, MessageHandler};
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
