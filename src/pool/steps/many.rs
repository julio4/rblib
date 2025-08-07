use {
	super::{select::PoolsDemux, *},
	crate::{alloy, reth},
	alloy::{consensus::Transaction, primitives::B256},
	dashmap::DashSet,
	reth::ethereum::primitives::SignedTransaction,
	std::{collections::HashSet, sync::Arc},
};

/// This step will append many new orders from the enabled pools to the end of
/// the current payload. Currently, it supports transactions and bundles and
/// queries the reth node transaction pool and optionally an instance of
/// `OrderPool`.
///
/// It will append new orders until either the payload limit, or the pool is
/// exhausted.
///
/// Unlike [`AppendOneOrder`], this step will not return `ControlFlow::Break`
/// when the pool is exhausted or payload limits are reached. Instead, it will
/// return `ControlFlow::Ok` with the modified payload.
pub struct AppendManyOrders<P: Platform> {
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

	/// Specifies the maximum number of new orders that will be added to the
	/// payload in one run of this step.
	max_new_orders: Option<usize>,

	/// Specifies the maximum number of new transactions that will be added to
	/// the payload across all new orders in one run of this step.
	max_new_transactions: Option<usize>,
}

impl<P: Platform> Default for AppendManyOrders<P> {
	/// The default configuration for this step will only pull orders from the
	/// system transaction pool hosted by Reth.
	fn default() -> Self {
		Self {
			order_pool: None,
			attempted: DashSet::new(),
			enable_system_pool: true,
			max_new_orders: None,
			max_new_transactions: None,
		}
	}
}

impl<P: Platform> AppendManyOrders<P> {
	/// Attaches this step a an `OrderPool` that supports bundles.
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self {
			order_pool: Some(pool.clone()),
			..Default::default()
		}
	}

	/// No transactions from the transaction pool running within the Reth node
	/// will be polled.
	#[must_use]
	pub fn disable_system_pool(mut self) -> Self {
		self.enable_system_pool = false;
		self
	}

	/// Specifies the maximum number of new orders that will be added to the
	/// payload in one run of this step.
	#[must_use]
	pub fn with_max_new_orders(mut self, max_new_orders: usize) -> Self {
		self.max_new_orders = Some(max_new_orders);
		self
	}

	/// Specifies the maximum number of new transactions that will be added to
	/// the payload across all new orders in one run of this step.
	#[must_use]
	pub fn with_max_new_transactions(
		mut self,
		max_new_transactions: usize,
	) -> Self {
		self.max_new_transactions = Some(max_new_transactions);
		self
	}
}

impl<P: Platform> Step<P> for AppendManyOrders<P> {
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
		let mut payload = payload;

		if payload.cumulative_gas_used() >= ctx.limits().gas_limit {
			// If the payload already at capacity, we can stop here.
			return ControlFlow::Ok(payload);
		}

		// Create an iterator that will demultiplex orders coming from the
		// orders pool that supports bundles and the system transaction pool.
		let mut orders = PoolsDemux::new(
			self.enable_system_pool.then(|| ctx.pool()),
			self.order_pool.as_ref(),
			ctx.block(),
		);

		let mut orders_added = 0;
		let mut transactions_added = 0;

		// the maximum number of new transactions that can be added to the
		// payload in this step. This will be None if no limits are set on
		// the number of new transactions and typical payload limits apply.
		let max_new_transactions =
			[self.max_new_transactions, ctx.limits().max_transactions]
				.into_iter()
				.flatten()
				.min();

		loop {
			if let Some(max_orders) = self.max_new_orders
				&& orders_added >= max_orders
			{
				// We've reached the maximum number of new orders to add to the payload.
				return ControlFlow::Ok(payload);
			}

			if let Some(max_new_transactions) = max_new_transactions
				&& transactions_added >= max_new_transactions
			{
				// We've reached the maximum number of new transactions to add to the
				// payload.
				return ControlFlow::Ok(payload);
			}

			let existing_txs: HashSet<_> = payload
				.history()
				.transactions()
				.map(|tx| tx.tx_hash())
				.copied()
				.collect();

			// pull next order
			let Some(order) = orders.next() else {
				// No more orders in the pool to add to the payload.
				return ControlFlow::Ok(payload);
			};

			let order_hash = order.hash();

			// tell the order pool that there was an inclusion attempt for this
			// order, this will help the pool make better decisions in future
			// `best_orders()` calls.
			ctx.emit(OrderInclusionAttempt(order_hash, ctx.block().payload_id()));

			if self.attempted.contains(&order_hash) {
				// This order was already attempted to be added to the payload for this
				// payload job.
				continue;
			}

			// mark this order as attempted to avoid adding it again for the same
			// payload job.
			self.attempted.insert(order_hash);

			if let Some(tx_count_limit) = max_new_transactions {
				if order.transactions().len() + transactions_added > tx_count_limit {
					// This order has too many transactions to fit in the remaining
					// transaction limit for this step, skip it.
					continue;
				}
			}

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
				// Including this order would exceed the gas limit for the payload.
				continue;
			}

			orders_added += 1;
			transactions_added += new_payload.transactions().len();
			ctx.emit(OrderInclusionSuccess(order_hash, ctx.block().payload_id()));

			// all good, extend the payload with the new checkpoint
			payload = new_payload;
		}
	}
}
