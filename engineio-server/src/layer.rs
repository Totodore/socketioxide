use tower::Layer;

use crate::service::EngineIoService;

#[derive(Debug, Clone)]
pub struct EngineIoLayer {
}

impl EngineIoLayer {
	pub fn new() -> Self {
		Self {}
	}
}

impl<S> Layer<S> for EngineIoLayer {
	type Service = EngineIoService<S>;

	fn layer(&self, inner: S) -> Self::Service {
		EngineIoService::new(inner)
	}
}