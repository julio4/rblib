use {crate::prelude::*, std::sync::Arc};

pub struct BuilderEpilogue;
impl<P: Platform> Step<P> for BuilderEpilogue {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		tracing::error!("Builder epilogue step is not implemented yet");
		ControlFlow::Ok(payload)
	}
}
