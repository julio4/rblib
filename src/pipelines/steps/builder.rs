use crate::*;

pub struct BuilderEpilogue;
impl Step for BuilderEpilogue {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!()
	}
}
