use {crate::*, std::sync::Arc};

pub struct BuilderEpilogue;
impl<P: Platform> Step<P> for BuilderEpilogue {
	type Kind = Simulated;

	async fn step(
		self: Arc<Self>,
		_payload: SimulatedPayload<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!()
	}
}
