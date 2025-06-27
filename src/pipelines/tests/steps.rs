use crate::{pipelines::StepContext, *};

pub struct OptimismPrologue;
impl Step for OptimismPrologue {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Static> {
		todo!()
	}
}

pub struct BuilderEpilogue;
impl Step for BuilderEpilogue {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct GatherBestTransactions;
impl Step for GatherBestTransactions {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Static> {
		// we need here access to the transaction pool
		todo!("Gathering best transactions from the pool")
	}
}

pub struct PriorityFeeOrdering;
impl Step for PriorityFeeOrdering {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Static> {
		todo!()
	}
}

pub struct TotalProfitOrdering;
impl Step for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct RevertProtection;
impl Step for RevertProtection {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct AppendNewTransactionFromPool;
impl Step for AppendNewTransactionFromPool {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload,
		_ctx: &StepContext<P>,
	) -> ControlFlow<Static> {
		todo!()
	}
}
