use {crate::*, std::sync::Arc};

pub struct TotalProfitOrdering;
impl<P: Platform> Step<P> for TotalProfitOrdering {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Ok(payload)
	}
}
