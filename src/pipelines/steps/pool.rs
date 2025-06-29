use {
	crate::*,
	alloy::consensus::Transaction,
	reth_transaction_pool::{
		error::{Eip4844PoolTransactionError, InvalidPoolTransactionError},
		PoolTransaction,
		TransactionPool,
	},
};

pub struct GatherBestTransactions;
impl Step for GatherBestTransactions {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		payload: SimulatedPayload<P>,
		ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		let mut txs = ctx.pool().best_transactions();
		let block_gas_limit = ctx.limits().gas_limit();

		let mut payload = payload;

		// This loop will keep populating the payload with new transactions from the
		// pool until either we exhaust the pool or we reach the limits of the
		// payload as defined in the pipeline.

		loop {
			// history is the ordered list of all transactions that were applied since
			// the beginning of the payload of the block under construction.
			let history = payload.history();

			if history.gas_used() >= block_gas_limit {
				// We've already reached the gas limit.
				return ControlFlow::Ok(payload);
			}

			if let Some(n_limit) = ctx.limits().max_transactions() {
				if history.len() >= n_limit {
					// We've reached the maximum number of txs in the block.
					// This is not a standard ethereum limit, but it may be configured
					// by the [`Limits`] trait implementation.
					return ControlFlow::Ok(payload);
				}
			}

			let Some(candidate) = txs.next() else {
				// No more transactions in the pool to add to the payload.
				return ControlFlow::Continue(payload);
			};

			let transaction =
				candidate.transaction.clone_into_consensus().into_inner();

			// if this is a blob transaction, apply blob limits
			if let Some(blob_gas) = transaction.blob_gas_used() {
				if blob_gas + history.blob_gas_used()
					> ctx.limits().max_total_blobs_size()
				{
					// we can't fit this transaction into the payload, because we are at
					// capacity for blobs in this payload.
					txs.mark_invalid(
						&candidate,
						InvalidPoolTransactionError::Eip4844(
							Eip4844PoolTransactionError::TooManyEip4844Blobs {
								have: history.blobs().count() as u64,
								permitted: ctx.limits().max_blob_transactions(),
							},
						),
					);
					continue;
				}
			}

			// we could potentially fit this transaction into the payload

			// create a new state checkpoint with the new transaction candidate
			// applied to it and check if we still fit within the gas limit.
			let new_payload = match payload
				.apply(candidate.transaction.clone_into_consensus().into_inner())
			{
				Ok(payload) => payload,
				Err(err) => return err.into(),
			};

			// check the gas used with the new transaction applied
			let new_gas_used = new_payload.history().gas_used();

			// we can't fit this transaction into the payload, skip it
			// also mark it as invalid to invalidate all descendat and dependant
			// transactions, then try the next transaction in the pool that might
			// fit in the remaining gas limit.
			if new_gas_used > block_gas_limit {
				txs.mark_invalid(
					&candidate,
					InvalidPoolTransactionError::ExceedsGasLimit(
						candidate.gas_limit(),
						block_gas_limit,
					),
				);
			}

			payload = new_payload;
		}
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
