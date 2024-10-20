//! ## A Tower [`Service`](tower::Service) and Hyper [`Service`](hyper::service::Service) for socket.io so it
//! can be used with frameworks supporting tower and hyper services.
//!
//! #### Example with a raw `hyper` standalone service (most of the time it easier to use a framework like `axum` or `salvo`):
//!
//! ```no_run
//! # use socketioxide::SocketIo;
//! # use std::net::SocketAddr;
//! # use hyper::server::conn::http1;
//! # use tokio::net::TcpListener;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (svc, io) = SocketIo::new_svc();
//!
//!      // Add io namespaces and events...
//!
//!      // Spawn raw hyper server
//!      let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//!      let listener = TcpListener::bind(addr).await.unwrap();
//!
//!      // We start a loop to continuously accept incoming connections
//!      loop {
//!          let (stream, _) = listener.accept().await.unwrap();
//!
//!          // Use an adapter to access something implementing `tokio::io` traits as if they implement
//!          // `hyper::rt` IO traits.
//!          let io = hyper_util::rt::TokioIo::new(stream);
//!          let svc = svc.clone();
//!
//!          // Spawn a tokio task to serve multiple connections concurrently
//!          tokio::task::spawn(async move {
//!              // Finally, we bind the incoming connection to our `hello` service
//!              if let Err(err) = http1::Builder::new()
//!                  .serve_connection(io, svc)
//!                  .with_upgrades()
//!                  .await
//!              {
//!                  println!("Error serving connection: {:?}", err);
//!             }
//!         });
//!     }
//! }
//! ```

use engineioxide::service::{EngineIoService, MakeEngineIoService};
use http::{Request, Response};
use http_body::Body;
use hyper::service::Service as HyperSvc;
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower_service::Service as TowerSvc;

use crate::{
    adapter::{Adapter, LocalAdapter},
    client::Client,
    SocketIoConfig,
};

/// A [`Tower`](TowerSvc)/[`Hyper`](HyperSvc) Service that wraps [`EngineIoService`] and
/// redirect every request to it
pub struct SocketIoService<S: Clone, A: Adapter = LocalAdapter> {
    engine_svc: EngineIoService<Client<A>, S>,
}

/// Tower Service implementation.
impl<S, ReqBody, ResBody, A> TowerSvc<Request<ReqBody>> for SocketIoService<S, A>
where
    ReqBody: Body + Send + Unpin + std::fmt::Debug + 'static,
    <ReqBody as Body>::Error: std::fmt::Debug,
    <ReqBody as Body>::Data: Send,
    ResBody: Body + Send + 'static,
    S: TowerSvc<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    A: Adapter,
{
    type Response = <EngineIoService<Client<A>, S> as TowerSvc<Request<ReqBody>>>::Response;
    type Error = <EngineIoService<Client<A>, S> as TowerSvc<Request<ReqBody>>>::Error;
    type Future = <EngineIoService<Client<A>, S> as TowerSvc<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.engine_svc.poll_ready(cx)
    }
    #[inline(always)]
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

/// Hyper 1.0 Service implementation.
impl<S, ReqBody, ResBody, A> HyperSvc<Request<ReqBody>> for SocketIoService<S, A>
where
    ReqBody: Body + Send + Unpin + std::fmt::Debug + 'static,
    <ReqBody as Body>::Error: std::fmt::Debug,
    <ReqBody as Body>::Data: Send,
    ResBody: Body + Send + 'static,
    S: HyperSvc<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    A: Adapter,
{
    type Response = <EngineIoService<Client<A>, S> as HyperSvc<Request<ReqBody>>>::Response;
    type Error = <EngineIoService<Client<A>, S> as HyperSvc<Request<ReqBody>>>::Error;
    type Future = <EngineIoService<Client<A>, S> as HyperSvc<Request<ReqBody>>>::Future;

    #[inline(always)]
    fn call(&self, req: Request<ReqBody>) -> Self::Future {
        self.engine_svc.call(req)
    }
}

impl<A: Adapter, S: Clone> SocketIoService<S, A> {
    /// Creates a MakeService which can be used as a hyper service
    #[inline(always)]
    pub fn into_make_service(self) -> MakeEngineIoService<Client<A>, S> {
        self.engine_svc.into_make_service()
    }

    /// Creates a new [`EngineIoService`] with a custom inner service and a custom config.
    pub(crate) fn with_config_inner(
        inner: S,
        config: SocketIoConfig,
        #[cfg(feature = "state")] state: state::TypeMap![Send + Sync],
    ) -> (Self, Arc<Client<A>>) {
        let engine_config = config.engine_config.clone();
        let client = Arc::new(Client::new(
            config,
            #[cfg(feature = "state")]
            state,
        ));
        let svc = EngineIoService::with_config_inner(inner, client.clone(), engine_config);
        (Self { engine_svc: svc }, client)
    }

    /// Creates a new [`EngineIoService`] with a custom inner service and an existing client
    /// It is mainly used with a [`SocketIoLayer`](crate::layer::SocketIoLayer) that owns the client
    pub(crate) fn with_client(inner: S, client: Arc<Client<A>>) -> Self {
        let engine_config = client.config.engine_config.clone();
        let svc = EngineIoService::with_config_inner(inner, client, engine_config);
        Self { engine_svc: svc }
    }
}

impl<A: Adapter, S: Clone> Clone for SocketIoService<S, A> {
    fn clone(&self) -> Self {
        Self {
            engine_svc: self.engine_svc.clone(),
        }
    }
}
