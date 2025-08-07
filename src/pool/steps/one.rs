use {
	super::{select::PoolsDemux, *},
	crate::{alloy, reth},
	alloy::{consensus::Transaction, primitives::B256},
	dashmap::DashSet,
	reth::ethereum::primitives::SignedTransaction,
	std::{collections::HashSet, sync::Arc},
	tracing::debug,
};

/// This step will append one new order from the enabled pools to the end of the
/// current payload. Currently, it supports transactions and bundles and queries
/// the reth node transaction pool and optionally an instance of `OrderPool`.
///
/// It will only append a new order if the current payload remains within the
/// payload limits of the pipeline after the order is applied.
///
/// If there are no more orders in the pool to append or the payload limit is
/// reached, it will return `ControlFlow::Break` with the unmodified input
/// payload.
///
/// This step is most useful in `Loop` pipelines, where it can be used to
/// append new orders to the payload in each iteration of the loop until
/// all pools are exhausted or the payload limits are reached.
pub struct AppendOneOrder<P: Platform> {
	/// An optional instance of the `OrderPool` that supports bundles.
	/// If this is `None`, only the system transaction pool that supports only
	/// loose transactions will be used.
	order_pool: Option<OrderPool<P>>,

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
	attempted: DashSet<B256>,

	/// When enabled, this step will also pull orders from the system transaction
	/// pool that is running within the Reth node. By default this is enabled.
	enable_system_pool: bool,
}

impl<P: Platform> Default for AppendOneOrder<P> {
	/// The default configuration for this step will only pull orders from the
	/// system transaction pool hosted by Reth.
	fn default() -> Self {
		Self {
			order_pool: None,
			attempted: DashSet::new(),
			enable_system_pool: true,
		}
	}
}

impl<P: Platform> AppendOneOrder<P> {
	/// Attaches this step a an `OrderPool` that supports bundles.
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self {
			order_pool: Some(pool.clone()),
			attempted: DashSet::new(),
			enable_system_pool: true,
		}
	}

	/// No transactions from the transaction pool running within the Reth node
	/// will be polled.
	#[must_use]
	pub fn disable_system_pool(mut self) -> Self {
		self.enable_system_pool = false;
		self
	}
}

impl<P: Platform> Step<P> for AppendOneOrder<P> {
	async fn before_job(
		self: Arc<Self>,
		_: StepContext<P>,
	) -> Result<(), PayloadBuilderError> {
		// Clear the list of attempted orders for this payload job.
		self.attempted.clear();
		Ok(())
	}

	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		if payload.cumulative_gas_used() >= ctx.limits().gas_limit {
			// We've already reached the gas limit.
			return ControlFlow::Break(payload);
		}

		// Create an iterator that will demultiplex orders coming from the
		// orders pool that supports bundles and the system transaction pool.
		let mut orders = PoolsDemux::new(
			self.enable_system_pool.then(|| ctx.pool()),
			self.order_pool.as_ref(),
			ctx.block(),
		);

		loop {
			// check if we have reached the deadline
			if ctx.limits().deadline_reached() {
				// stop the loop and return the current payload
				debug!(
					"Payload building deadline reached for {}",
					ctx.block().payload_id()
				);
				return ControlFlow::Break(payload);
			}

			let existing_txs: HashSet<_> = payload
				.history()
				.transactions()
				.map(|tx| tx.tx_hash())
				.copied()
				.collect();

			if let Some(n_limit) = ctx.limits().max_transactions {
				if existing_txs.len() >= n_limit {
					// We've reached the maximum number of txs in the block.
					// This is not a standard ethereum limit, but it may be configured
					// by the [`Limits`] trait implementation.
					return ControlFlow::Break(payload);
				}
			}

			// pull next order
			let Some(order) = orders.next() else {
				// No more orders in the pool to add to the payload.
				return ControlFlow::Break(payload);
			};

			let order_hash = order.hash();

			if self.attempted.contains(&order_hash) {
				// This order was already attempted to be added to the payload for this
				// payload job.
				continue;
			}

			// tell the order pool that there was an inclusion attempt for this
			// order, this will help the pool make better decisions in future
			// `best_orders()` calls.
			ctx.emit(OrderInclusionAttempt(order_hash, ctx.block().payload_id()));

			// mark this order as attempted to avoid adding it again for the same
			// payload job.
			self.attempted.insert(order_hash);

			// check if any of the transactions in the order are already in the
			// payload
			if order
				.transactions()
				.iter()
				.any(|tx| existing_txs.contains(tx.tx_hash()))
			{
				// this order is not eligible for inclusion in the payload because it
				// contains transactions that are already included in the payload.
				continue;
			}

			let order_blob_gas = order
				.transactions()
				.iter()
				.filter_map(|tx| tx.blob_gas_used())
				.sum::<u64>();

			if let Some(blob_limits) = ctx.limits().blob_params {
				let cumulative = order_blob_gas + payload.cumulative_blob_gas_used();
				if cumulative > blob_limits.max_blob_gas_per_block() {
					// we can't fit this order into the payload, because we are at
					// capacity for blobs in this payload.
					continue;
				}
			}

			let executable = match order.try_into_executable() {
				Ok(executable) => executable,
				Err(err) => {
					// Order has transactions that cannot have their signers recovered.
					ctx.emit(OrderInclusionFailure::<P>(
						order_hash,
						ExecutionError::InvalidSignature(err).into(),
						ctx.block().payload_id(),
					));
					continue;
				}
			};

			// try to create a new payload checkpoint with the order, we could
			// potentially fit this order into the payload, but we need to check if
			// it fits within the gas limit.
			let new_payload = match payload.apply(executable) {
				Ok(checkpoint) => checkpoint,
				Err(err) => {
					// skip this order, it cannot be applied to the payload and let the
					// order pool know about this failure so it can make better
					// decisions in the future when it returns the best orders
					// iterator.
					ctx.emit(OrderInclusionFailure(
						order_hash,
						err.into(),
						ctx.block().payload_id(),
					));
					continue;
				}
			};

			if new_payload.cumulative_gas_used() > ctx.limits().gas_limit {
				// Including this order would exceed the gas limit for the payload,
				// skip it, there may be other smaller orders that can fit in the
				// remaining gas limit.
				continue;
			}

			ctx.emit(OrderInclusionSuccess(order_hash, ctx.block().payload_id()));
			return ControlFlow::Ok(new_payload);
		}
	}
}
