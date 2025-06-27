use crate::*;

pub struct OptimismPrologue;
impl Step for OptimismPrologue {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Static> {
		todo!()
	}
}
