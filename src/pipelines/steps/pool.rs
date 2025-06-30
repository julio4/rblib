use {
	crate::*,
	alloy::consensus::Transaction,
	reth_transaction_pool::{
		error::{Eip4844PoolTransactionError, InvalidPoolTransactionError},
		PoolTransaction,
		TransactionPool,
	},
	std::time::Instant,
};

/// This step will gather best transactions from the pool and apply them to the
/// payload under construction. New transactions will be added until either the
/// payload limits are reached or the pool is exhausted.
pub struct GatherBestTransactions;
impl Step for GatherBestTransactions {
	type Kind = Simulated;

	async fn step<P: Platform>(
		&mut self,
		payload: SimulatedPayload<P>,
		ctx: &StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		let mut payload = payload;
		let mut txs = ctx.pool().best_transactions();

		// This loop will keep populating the payload with new transactions from the
		// pool until either we exhaust the pool or we reach the limits of the
		// payload as defined in the pipeline.

		loop {
			// check if we have reached the deadline
			if let Some(deadline) = ctx.limits().deadline {
				if deadline >= Instant::now() {
					// We have reached the deadline, stop this step.
					return ControlFlow::Ok(payload);
				}
			}

			// history is the ordered list of all transactions that were applied since
			// the beginning of the payload of the block under construction.
			let history = payload.history();

			if history.gas_used() >= ctx.limits().gas_limit {
				// We've already reached the gas limit.
				return ControlFlow::Ok(payload);
			}

			if let Some(n_limit) = ctx.limits().max_transactions {
				if history.transactions().count() >= n_limit {
					// We've reached the maximum number of txs in the block.
					// This is not a standard ethereum limit, but it may be configured
					// by the [`Limits`] trait implementation.
					return ControlFlow::Ok(payload);
				}
			}

			let Some(candidate) = txs.next() else {
				// No more transactions in the pool to add to the payload.
				return ControlFlow::Ok(payload);
			};

			let transaction =
				candidate.transaction.clone_into_consensus().into_inner();

			// if this is a blob transaction, and we have blob limits,
			// check if we can fit it into the payload.
			if let Some((blob_gas, blob_limits)) = transaction
				.blob_gas_used()
				.and_then(|g| ctx.limits().blob_params.map(|p| (g, p)))
			{
				if blob_gas + history.blob_gas_used()
					> blob_limits.max_blob_gas_per_block()
				{
					// we can't fit this transaction into the payload, because we are at
					// capacity for blobs in this payload.
					txs.mark_invalid(
						&candidate,
						InvalidPoolTransactionError::Eip4844(
							Eip4844PoolTransactionError::TooManyEip4844Blobs {
								have: history.blobs().count() as u64,
								permitted: blob_limits.max_blob_count,
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

			// check the cumulative gas used with the new transaction applied
			let new_gas_used = new_payload.history().gas_used();

			// we can't fit this transaction into the payload, skip it
			// also mark it as invalid to invalidate all descendant and dependent
			// transactions, then try the next transaction in the pool that might
			// fit in the remaining gas limit.
			if new_gas_used > ctx.limits().gas_limit {
				txs.mark_invalid(
					&candidate,
					InvalidPoolTransactionError::ExceedsGasLimit(
						candidate.gas_limit(),
						ctx.limits().gas_limit,
					),
				);
				continue;
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
