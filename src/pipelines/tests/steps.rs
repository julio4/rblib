use {
	crate::{pipelines::StepContext, *},
	reth_transaction_pool::TransactionPool,
	tracing::info,
};

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
		ctx: &StepContext<P>,
	) -> ControlFlow<Static> {
		let size = ctx.pool().pool_size();
		let txs = ctx.pool().all_transactions();
		info!(">---> Transaction pool size: {size:?}");
		info!(">---> Transaction pool transactions: {txs:?}");
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
