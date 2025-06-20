use crate::*;

pub struct OptimismPrologue;
impl Step for OptimismPrologue {
	type Kind = Static;

	async fn step(
		&mut self,
		_payload: StaticPayload,
		_ctx: &mut StaticContext,
	) -> ControlFlow<Static> {
		todo!()
	}
}

pub struct BuilderEpilogue;
impl Step for BuilderEpilogue {
	type Kind = Simulated;

	async fn step(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &mut SimulatedContext,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct GatherBestTransactions;
impl Step for GatherBestTransactions {
	type Kind = Static;

	async fn step(
		&mut self,
		_payload: StaticPayload,
		_ctx: &mut StaticContext,
	) -> ControlFlow<Static> {
		todo!()
	}
}

pub struct PriorityFeeOrdering;
impl Step for PriorityFeeOrdering {
	type Kind = Static;

	async fn step(
		&mut self,
		_payload: StaticPayload,
		_ctx: &mut StaticContext,
	) -> ControlFlow<Static> {
		todo!()
	}
}

pub struct TotalProfitOrdering;
impl Step for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &mut SimulatedContext,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct RevertProtection;
impl Step for RevertProtection {
	type Kind = Simulated;

	async fn step(
		&mut self,
		_payload: SimulatedPayload,
		_ctx: &mut SimulatedContext,
	) -> ControlFlow<Simulated> {
		todo!()
	}
}

pub struct AppendNewTransactionFromPool;
impl Step for AppendNewTransactionFromPool {
	type Kind = Static;

	async fn step(
		&mut self,
		_payload: StaticPayload,
		_ctx: &mut StaticContext,
	) -> ControlFlow<Static> {
		todo!()
	}
}
