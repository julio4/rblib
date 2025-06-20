use crate::*;

pub struct OptimismPrologue;
impl Step for OptimismPrologue {
	type Mode = Static;

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
	type Mode = Simulated;

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
	type Mode = Static;

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
	type Mode = Static;

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
	type Mode = Simulated;

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
	type Mode = Simulated;

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
	type Mode = Static;

	async fn step(
		&mut self,
		_payload: StaticPayload,
		_ctx: &mut StaticContext,
	) -> ControlFlow<Static> {
		todo!()
	}
}
