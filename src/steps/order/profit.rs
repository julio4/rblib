use {crate::prelude::*, std::sync::Arc};

pub struct OrderByTotalProfit;
impl<P: Platform> Step<P> for OrderByTotalProfit {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Ok(payload)
	}
}
