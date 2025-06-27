use {crate::*, reth_transaction_pool::TransactionPool, tracing::info};

pub struct GatherBestTransactions;
impl Step for GatherBestTransactions {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload<P>,
		ctx: &StepContext<P>,
	) -> ControlFlow<P, Static> {
		let size = ctx.pool().pool_size();
		let txs = ctx.pool().all_transactions();

		info!(">---> Transaction pool size: {size:?}");
		info!(">---> Transaction pool transactions: {txs:?}");
		todo!("Gathering best transactions from the pool")
	}
}

pub struct AppendNewTransactionFromPool;
impl Step for AppendNewTransactionFromPool {
	type Kind = Static;

	async fn step<P: Platform>(
		&mut self,
		_payload: StaticPayload<P>,
		_ctx: &StepContext<P>,
	) -> ControlFlow<P, Static> {
		todo!("Append new transaction from pool")
	}
}
