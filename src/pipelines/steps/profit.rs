use crate::*;

pub struct PriorityFeeOrdering;
impl Step for PriorityFeeOrdering {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Static> {
		todo!()
	}
}

pub struct TotalProfitOrdering;
impl Step for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!()
	}
}
