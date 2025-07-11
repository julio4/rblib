use {
	crate::*,
	alloy::{consensus::Transaction, primitives::TxHash},
	dashmap::DashSet,
	reth_ethereum::primitives::SignedTransaction,
	reth_payload_builder::PayloadBuilderError,
	reth_transaction_pool::{
		PoolTransaction,
		TransactionPool,
		error::{
			Eip4844PoolTransactionError,
			InvalidPoolTransactionError,
			PoolTransactionError,
		},
	},
	std::{collections::HashSet, sync::Arc, time::Instant},
};

/// This step will append one new transaction from the pool to the current
/// payload.
///
/// It will only append a new transaction if the current payload
/// remains within the payload limits of the pipeline after the transaction is
/// applied and return `ControlFlow::Ok` with the new payload, otherwise it will
/// return `ControlFlow::Break` with the payload prior to the transaction
/// application.
///
/// It will return `ControlFlow::Break` if there are no more transactions in the
/// pool to append.
///
/// It will not append transactions that are already in the payload.
///
/// This step is most useful in `Loop` pipelines, where it can be used to
/// append new transactions to the payload in each iteration of the loop until
/// the pool is exhausted or the payload limits are reached.
#[derive(Default)]
pub struct AppendNewTransactionFromPool {
	/// Keeps track of the transactions that were added to the payload in this
	/// payload building run. This is used to avoid infinite loops where some
	/// future step of the pipeline removed a previously added transaction from
	/// the payload and in the next iteration of the loop this will try to add
	/// it again and cause an infinite loop.
	///
	/// This is especially visible in the following scenario:
	///
	/// Loop:
	///  - `AppendNewTransactionFromPool`, adds tx A and B
	///  - `RevertProtection`, Removes tx B,
	///  - `AppendNewTransactionFromPool`, adds B
	///  - `RevertProtection`, Removed tx B,
	///  - `AppendNewTransactionFromPool`, adds B, etc.
	///
	/// This list is cleared at the end of each payload job.
	previously_added: DashSet<TxHash>,
}

impl<P: Platform> Step<P> for AppendNewTransactionFromPool {
	/// Clear the list of previously added transactions before the we begin
	/// building for a new payload.
	async fn before_job(
		self: Arc<Self>,
		_: Arc<StepContext<P>>,
	) -> Result<(), PayloadBuilderError> {
		self.previously_added.clear();
		Ok(())
	}

	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let mut txs = ctx.pool().best_transactions();

		// history is the ordered list of all transactions that were applied since
		// the beginning of the payload of the block under construction.
		let history = payload.history();

		if history.gas_used() >= ctx.limits().gas_limit {
			// We've already reached the gas limit.
			return ControlFlow::Break(payload);
		}

		if let Some(n_limit) = ctx.limits().max_transactions {
			if history.transactions().count() >= n_limit {
				// We've reached the maximum number of txs in the block.
				// This is not a standard ethereum limit, but it may be configured
				// by the [`Limits`] trait implementation.
				return ControlFlow::Break(payload);
			}
		}

		let existing_txs: HashSet<_> = payload
			.history()
			.transactions()
			.map(|tx| tx.tx_hash())
			.copied()
			.collect();

		loop {
			// check if we have reached the deadline
			if let Some(deadline) = ctx.limits().deadline {
				if deadline >= Instant::now() {
					// stop the loop and return the current payload
					return ControlFlow::Break(payload);
				}
			}

			let Some(candidate) = txs.next() else {
				// No more transactions in the pool to add to the payload.
				return ControlFlow::Break(payload);
			};

			if existing_txs.contains(candidate.hash()) {
				// This transaction is already in the payload, skip it.
				continue;
			}

			if self.previously_added.contains(candidate.hash()) {
				// This transaction was added to the payload in this
				// payload building run, but was removed by some other step
				// later in the pipeline. mark it as invalid to avoid adding
				// dependent transactions.
				txs.mark_invalid(
					&candidate,
					InvalidPoolTransactionError::Other(
						DiscardedByOtherSteps(*candidate.hash()).boxed(),
					),
				);
				continue;
			}

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

			// we successfully applied the transaction to the payload, so we can
			// mark it as previously added to avoid adding it again in the next
			// iteration of the loop.
			self.previously_added.insert(*candidate.hash());

			// we successfully applied the transaction to the payload, return the new
			// payload with the transaction included.
			return ControlFlow::Ok(new_payload);
		}
	}
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("Transaction was discarded by other steps in the pipeline: {0}")]
pub struct DiscardedByOtherSteps(pub TxHash);

impl DiscardedByOtherSteps {
	pub fn boxed(self) -> Box<dyn PoolTransactionError> {
		Box::new(self)
	}
}

impl PoolTransactionError for DiscardedByOtherSteps {
	fn is_bad_transaction(&self) -> bool {
		false
	}

	fn as_any(&self) -> &dyn std::any::Any {
		self
	}
}
