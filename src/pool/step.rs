use {
	super::{select::PoolsDemux, *},
	crate::{alloy, reth},
	alloy::{consensus::Transaction, primitives::B256},
	dashmap::DashSet,
	reth::{
		ethereum::primitives::SignedTransaction,
		payload::builder::PayloadId,
	},
	std::{collections::HashSet, sync::Arc},
};

/// This step will append new orders from the enabled pools to the end of
/// the current payload. Currently, it supports transactions and bundles and
/// queries the reth node transaction pool and optionally an instance of
/// `OrderPool`.
///
/// It will append new orders until either the payload limit, or the pool is
/// exhausted or one of the configured limits is reached.
pub struct AppendOrders<P: Platform> {
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

	/// Specifies the maximum number of new bundles that will be added to the
	/// payload in one run of this step.
	max_new_bundles: Option<usize>,

	/// Specifies the maximum number of new transactions that will be added to
	/// the payload across all new orders in one run of this step.
	max_new_transactions: Option<usize>,

	/// Specifies whether this step should return `ControlFlow::Ok` or
	/// `ControlFlow::Break` when limits are reached and we were not able to add
	/// new transactions in this run of the step.
	///
	/// This option is meaningful when this step is used in `Loop`s.
	/// Defaults to `true`.
	break_on_limit: bool,
}

impl<P: Platform> Default for AppendOrders<P> {
	/// The default configuration for this step will only pull orders from the
	/// system transaction pool hosted by Reth.
	fn default() -> Self {
		Self {
			order_pool: None,
			attempted: DashSet::new(),
			enable_system_pool: true,
			max_new_orders: None,
			max_new_bundles: None,
			max_new_transactions: None,
			break_on_limit: true,
		}
	}
}

/// Construction
impl<P: Platform> AppendOrders<P> {
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

	/// Specifies the maximum number of new bundles that will be added to the
	/// payload in one run of this step.
	#[must_use]
	pub fn with_max_new_bundles(mut self, max_new_bundles: usize) -> Self {
		self.max_new_bundles = Some(max_new_bundles);
		self
	}

	/// Specifies the maximum number of new transactions that will be added to
	/// the payload across all new bundles in one run of this step.
	#[must_use]
	pub fn with_max_new_transactions(mut self, count: usize) -> Self {
		self.max_new_transactions = Some(count);
		self
	}

	/// Specifies whether this step should return `ControlFlow::Ok` or
	/// `ControlFlow::Break` when payload limits are reached and no new orders
	/// can be added to the payload.
	#[must_use]
	pub fn with_ok_on_limit(mut self) -> Self {
		self.break_on_limit = false;
		self
	}
}

impl<P: Platform> Step<P> for AppendOrders<P> {
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
		let mut run = Run::new(&self, &ctx, payload);

		// Create an iterator that will demultiplex orders coming from the
		// orders pool that supports bundles and the system transaction pool.
		let mut orders = PoolsDemux::new(
			self.enable_system_pool.then(|| ctx.pool()),
			self.order_pool.as_ref(),
			ctx.block(),
		);

		loop {
			if run.should_stop() {
				// We have reached one of the limits
				return run.end();
			}

			// pull next order
			let Some(order) = orders.next() else {
				// No more orders in the pool to add to the payload.
				return run.end();
			};

			let order_hash = order.hash();

			// tell the order pool that there was an inclusion attempt for this
			// order, this will help the pool make better decisions in future
			// `best_orders()` calls.
			ctx.emit(OrderInclusionAttempt(order_hash, ctx.block().payload_id()));

			if !self.attempted.insert(order_hash) {
				// This order was already attempted to be added to the payload for this
				// payload job.
				continue;
			}

			run.try_include(order);
		}
	}
}

/// Holds the state of a single run of the `AppendManyOrders` step.
struct Run<'a, P: Platform> {
	step: &'a AppendOrders<P>,
	ctx: &'a StepContext<P>,
	payload: Checkpoint<P>,
	txs_included: usize,
	bundles_included: usize,
	orders_included: usize,
	max_transactions: Option<usize>,
	existing_txs: HashSet<TxHash>,
}

impl<'a, P: Platform> Run<'a, P> {
	fn new(
		step: &'a AppendOrders<P>,
		ctx: &'a StepContext<P>,
		payload: Checkpoint<P>,
	) -> Self {
		let limits = ctx.limits();
		let max_transactions = [step.max_new_transactions, limits.max_transactions]
			.into_iter()
			.flatten()
			.min();

		let existing_txs = payload
			.history()
			.transactions()
			.map(|tx| tx.tx_hash())
			.copied()
			.collect();

		Self {
			step,
			ctx,
			payload,
			txs_included: 0,
			orders_included: 0,
			bundles_included: 0,
			max_transactions,
			existing_txs,
		}
	}

	const fn limits(&self) -> &Limits {
		self.ctx.limits()
	}

	/// Returns `true` if the step should stop attempting to add new orders to the
	/// payload. This result means that no new orders can be added to the payload
	/// regardless of their content.
	fn should_stop(&self) -> bool {
		if self.limits().deadline_reached() {
			return true;
		}

		if let Some(max_orders) = self.step.max_new_orders
			&& self.orders_included >= max_orders
		{
			return true;
		}

		if let Some(max_transactions) = self.max_transactions
			&& self.txs_included >= max_transactions
		{
			return true;
		}

		if let Some(max_bundles) = self.step.max_new_bundles
			&& self.bundles_included >= max_bundles
		{
			return true;
		}

		if self.payload.cumulative_gas_used() >= self.limits().gas_limit {
			return true;
		}

		false
	}

	/// Returns `true` if the step should not attempt to add this specific order,
	/// but may include other orders that are more suitable.
	fn should_skip(&self, order: &Order<P>) -> bool {
		if let Some(max_transactions) = self.max_transactions {
			if order.transactions().len() + self.txs_included > max_transactions {
				// This order has too many transactions to fit in the remaining
				// transaction limit for this step, skip it.
				return true;
			}
		}

		if let Some(max_bundles) = self.step.max_new_bundles
			&& matches!(order, Order::Bundle(_))
			&& self.bundles_included >= max_bundles
		{
			// We're at limit for included bundles for this run of the step.
			return true;
		}

		if order
			.transactions()
			.iter()
			.any(|tx| self.existing_txs.contains(tx.tx_hash()))
		{
			// order contains transactions that are already included in the payload.
			return true;
		}

		let order_blob_gas = order
			.transactions()
			.iter()
			.filter_map(|tx| tx.blob_gas_used())
			.sum::<u64>();

		if let Some(blob_limits) = self.limits().blob_params
			&& order_blob_gas + self.payload.cumulative_blob_gas_used()
				> blob_limits.max_blob_gas_per_block()
		{
			// we can't fit this order into the payload, because we are at
			// capacity for blobs in this payload.
			return true;
		}

		false
	}

	/// Tries to extend the current payload with the contents of the given order.
	/// If the order is skipped, the payload checkpoint remains unchanged.
	pub fn try_include(&mut self, order: Order<P>) {
		if self.should_skip(&order) {
			return;
		}

		let order_hash = order.hash();
		let executable = match order.try_into_executable() {
			Ok(executable) => executable,
			Err(err) => {
				// Order has transactions that cannot have their signers recovered.
				return self.ctx.emit(OrderInclusionFailure::<P>(
					order_hash,
					ExecutionError::InvalidSignature(err).into(),
					self.ctx.block().payload_id(),
				));
			}
		};

		// try to create a new payload checkpoint with the order, we could
		// potentially fit this order into the payload, but we need to check if
		// it fits within the gas limit.
		let candidate = match self.payload.apply(executable) {
			Ok(checkpoint) => checkpoint,
			Err(err) => {
				// This order cannot be used to create a valid checkpoint.
				// skip it and notify the world about this inclusion failure.
				return self.ctx.emit(OrderInclusionFailure(
					order_hash,
					err.into(),
					self.ctx.block().payload_id(),
				));
			}
		};

		if candidate.cumulative_gas_used() > self.limits().gas_limit {
			// Including this order would exceed the gas limit for the payload,
			// skip it, and try other available orders that might fit within the
			// remaining gas budget.
			return;
		}

		// checkpoint is valid and fits within limits.

		self.orders_included += 1;
		self.txs_included += candidate.transactions().len();
		self.bundles_included += usize::from(candidate.is_bundle());

		self.payload = candidate;

		self.ctx.emit(OrderInclusionSuccess(
			order_hash,
			self.ctx.block().payload_id(),
		));
	}

	pub fn end(self) -> ControlFlow<P> {
		if self.step.break_on_limit && self.txs_included == 0 {
			ControlFlow::Break(self.payload)
		} else {
			ControlFlow::Ok(self.payload)
		}
	}
}

/// Event emitted when an order was considered for inclusion in a payload
#[derive(Debug, Clone)]
pub struct OrderInclusionAttempt(pub B256, pub PayloadId);

/// Event emitted when an order was successfully included in a payload.
#[derive(Debug, Clone)]
pub struct OrderInclusionSuccess(pub B256, pub PayloadId);

/// Event emitted when an order was proposed by the pool but it failed to create
/// a valid checkpoint.
#[derive(Debug, Clone)]
pub struct OrderInclusionFailure<P: Platform>(
	pub B256,
	pub Arc<ExecutionError<P>>,
	pub PayloadId,
);
