use {crate::*, std::sync::Arc};

pub struct PriorityFeeOrdering;
impl<P: Platform> Step<P> for PriorityFeeOrdering {
	type Kind = Static;

	async fn step(
		self: Arc<Self>,
		payload: StaticPayload<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P, Static> {
		todo!("priority fee ordering for {payload:?}")
	}
}

pub struct TotalProfitOrdering;
impl<P: Platform> Step<P> for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step(
		self: Arc<Self>,
		payload: SimulatedPayload<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!("total profit ordering for {payload:?}")
	}
}
