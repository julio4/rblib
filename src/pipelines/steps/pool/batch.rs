use {
	crate::{
		alloy::consensus::Transaction,
		reth::transaction_pool::TransactionPool,
		*,
	},
	std::{sync::Arc, time::Instant},
};

/// This step will gather best transactions from the pool and apply them to the
/// payload under construction. New transactions will be added until either the
/// payload limits are reached or the pool is exhausted.
pub struct GatherBestTransactions;
impl<P: Platform> Step<P> for GatherBestTransactions {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let mut payload = payload;
		let mut txs = ctx.pool().best_transactions();

		// This loop will keep populating the payload with new transactions from the
		// pool until either we exhaust the pool or we reach the limits of the
		// payload as defined in the pipeline.

		loop {
			// check if we have reached the deadline
			if let Some(deadline) = ctx.limits().deadline {
				if deadline <= Instant::now() {
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

			let transaction = candidate.transaction.clone();

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
					continue;
				}
			}

			// we could potentially fit this transaction into the payload,
			// create a new state checkpoint with the new transaction candidate
			// applied to it and check if we still fit within the gas limit.
			let Ok(new_payload) = payload.apply(transaction) else {
				// if the transaction cannot be applied because of evm consensus rules,
				// we skip it and move on to the next one.
				continue;
			};

			// check the cumulative gas used with the new transaction applied
			let new_gas_used = new_payload.history().gas_used();

			// we can't fit this transaction into the payload, skip it
			// also mark it as invalid to invalidate all descendant and dependent
			// transactions, then try the next transaction in the pool that might
			// fit in the remaining gas limit.
			if new_gas_used > ctx.limits().gas_limit {
				continue;
			}

			payload = new_payload;
		}
	}
}

#[cfg(test)]
mod tests {
	use {super::*, crate::test_utils::*};

	#[rblib_test(Ethereum, Optimism)]
	async fn empty_pool<P: TestablePlatform>() {
		let output = OneStep::<P>::new(GatherBestTransactions).run().await;
		let Ok(ControlFlow::Ok(payload)) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};
		assert_eq!(payload.history().transactions().count(), 0);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn one_transaction<P: TestablePlatform>() {
		let output = OneStep::<P>::new(GatherBestTransactions)
			.with_pool_tx(|builder| builder.transfer())
			.run()
			.await;
		let Ok(ControlFlow::Ok(payload)) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};
		assert_eq!(payload.history().transactions().count(), 1);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn many_transactions<P: TestablePlatform>() {
		const COUNT: usize = 15;
		let mut step = OneStep::<P>::new(GatherBestTransactions);

		for _ in 0..COUNT {
			step = step.with_pool_tx(|builder| builder.transfer());
		}

		let output = step.run().await;
		let Ok(ControlFlow::Ok(payload)) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().len(), 1 + COUNT);
		assert_eq!(payload.history().transactions().count(), COUNT);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn too_many_transactions<P: TestablePlatform>() {
		const COUNT: usize = 200;
		const EXPECTED: usize = 100;

		let mut output = OneStep::<P>::new(GatherBestTransactions)
			.with_limits(Limits::with_gas_limit(21000 * EXPECTED as u64));

		for _ in 0..COUNT {
			output = output.with_pool_tx(|builder| builder.transfer());
		}

		let output = output.run().await;
		let Ok(ControlFlow::Ok(payload)) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		// we should reach the gas limit before we include all transactions
		assert_eq!(payload.history().len(), 1 + EXPECTED);
		assert_eq!(payload.history().transactions().count(), EXPECTED);
	}
}
