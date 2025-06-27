use crate::*;

pub struct RevertProtection;
impl Step for RevertProtection {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!()
	}
}
