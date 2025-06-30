use crate::*;

pub struct PriorityFeeOrdering;
impl Step for PriorityFeeOrdering {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		payload: StaticPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Static> {
		todo!("priority fee ordering for {payload:?}")
	}
}

pub struct TotalProfitOrdering;
impl Step for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		payload: SimulatedPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!("total profit ordering for {payload:?}")
	}
}
