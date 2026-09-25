//! Functions and types used to handle incoming connections and messages.
//! There is three main types of handlers: [connect], [message] and [disconnect].
pub mod disconnect;
pub mod message;

pub(crate) use disconnect::BoxedDisconnectHandler;
pub(crate) use message::BoxedMessageHandler;

/// A struct used to erase the type of [`ConnectHandler`] or [`MessageHandler`] so it can be stored in a map
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
