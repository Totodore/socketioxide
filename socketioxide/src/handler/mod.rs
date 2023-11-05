mod connect;
mod message;

pub(crate) use connect::{BoxedConnectHandler, ConnectHandler};
pub use message::{AckResponse, AckSender};
pub(crate) use message::{BoxedMessageHandler, CallbackHandler};
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
