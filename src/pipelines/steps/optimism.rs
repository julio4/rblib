use {crate::*, std::sync::Arc};

// This also demonstrates how we can author steps that are specific to a
// platform

pub struct OptimismPrologue;
impl Step<Optimism> for OptimismPrologue {
	type Kind = Static;

	async fn step(
		self: Arc<Self>,
		_payload: StaticPayload<Optimism>,
		_ctx: StepContext<Optimism>,
	) -> ControlFlow<Optimism, Static> {
		todo!()
	}
}
