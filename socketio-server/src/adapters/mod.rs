#[cfg(feature = "async_adapter")]
pub use async_adapter::AsyncAdapter as Adapter;
#[cfg(feature = "async_adapter")]
mod async_adapter;

#[cfg(not(feature = "async_adapter"))]
pub use adapter::Adapter;
#[cfg(not(feature = "async_adapter"))]
mod adapter;
#[cfg(not(feature = "async_adapter"))]
mod local_adapter;
#[cfg(not(feature = "async_adapter"))]
pub use local_adapter::LocalAdapter;