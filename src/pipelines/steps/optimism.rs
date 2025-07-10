use {crate::*, std::sync::Arc};

// This also demonstrates how we can author steps that are specific to a
// platform

pub struct OptimismPrologue;
impl Step<Optimism> for OptimismPrologue {
	async fn step(
		self: Arc<Self>,
		_payload: Checkpoint<Optimism>,
		_ctx: StepContext<Optimism>,
	) -> ControlFlow<Optimism> {
		todo!()
	}
}
