use {
	super::{Order, OrderPool},
	crate::{alloy, prelude::*, reth},
	alloy::primitives::B256,
	dashmap::DashSet,
	reth::{
		ethereum::primitives::SignedTransaction,
		transaction_pool::TransactionPool,
	},
	serde::{Serialize, de::DeserializeOwned},
	std::{collections::HashSet, sync::Arc, time::Instant},
	tracing::debug,
};

pub struct AppendOneOrder<P: Platform>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pool: OrderPool<P>,

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

	/// If true, bundles that do not fit within the gas limit will be attempted
	/// to be included by removing some of their optional transactions. Default
	/// is false.
	partial_orders: bool,

	/// When enabled, the step will not pull any orders from the system
	/// transaction pool. Otherwise, when there are no orders in the
	/// `OrderPool`, it will pull orders from the system pool provided by reth.
	disable_system_pool: bool,
}

impl<P: Platform> AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self {
			pool: pool.clone(),
			attempted: DashSet::new(),
			partial_orders: false,
			disable_system_pool: false,
		}
	}

	/// For bundles that do not fit within the gas limit, attempt to include them
	/// by removing some of their optional transactions.
	///
	/// TODO: Implement this feature.
	#[must_use]
	pub fn allow_partial_orders(mut self) -> Self {
		self.partial_orders = true;
		self
	}

	/// When enabled, the step will not pull any orders from the system
	/// transaction pool. Otherwise, when there are no orders in the
	/// `OrderPool`, it will pull orders from the system pool provided by reth.
	#[must_use]
	pub fn disable_system_pool(mut self) -> Self {
		self.disable_system_pool = true;
		self
	}
}

impl<P: Platform> Step<P> for AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	async fn before_job(
		self: Arc<Self>,
		_: Arc<StepContext<P>>,
	) -> Result<(), PayloadBuilderError> {
		// Clear the list of attempted orders for this payload job.
		self.attempted.clear();
		Ok(())
	}

	async fn after_job(
		self: Arc<Self>,
		result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Result<(), PayloadBuilderError> {
		if let Ok(built_payload) = result.as_ref() {
			self.pool.report_produced_payload(built_payload);
		}
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

		let mut orders = self.pool.best_orders_for_block(ctx.block());
		let mut system_pool_txs = ctx.pool().best_transactions();

		loop {
			// check if we have reached the deadline
			if let Some(deadline) = ctx.limits().deadline {
				if deadline <= Instant::now() {
					// stop the loop and return the current payload
					debug!(
						"Payload building deadline reached for {}",
						ctx.block().payload_id()
					);
					return ControlFlow::Break(payload);
				}
			}

			// pull next order
			let order = if let Some(order) = orders.next() {
				order
			} else {
				// No more orders in the order pool, see if we are allowed to
				// fall back to the system transaction pool.
				if self.disable_system_pool {
					return ControlFlow::Break(payload);
				}

				match system_pool_txs.next() {
					Some(tx) => Order::Transaction(tx.to_consensus()),
					None => return ControlFlow::Break(payload),
				}
			};

			let order_hash = order.hash();

			// tell the order pool that there was an inclusion attempt for this
			// order, this will help the pool make better decisions in future
			// `best_orders()` calls.
			self.pool.report_inclusion_attempt(order_hash, ctx.block());

			if self.attempted.contains(&order_hash) {
				// This order was already attempted to be added to the payload for this
				// payload job.
				continue;
			}

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

			let executable = match order.try_into_executable() {
				Ok(executable) => executable,
				Err(err) => {
					// Order has transactions that cannot have their signers recovered.
					self.pool.report_execution_error(
						order_hash,
						ExecutionError::InvalidSignature(err),
					);
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
					self.pool.report_execution_error(order_hash, err);
					continue;
				}
			};

			if new_payload.cumulative_gas_used() > ctx.limits().gas_limit {
				// Including this order would exceed the gas limit for the payload.
				// TODO: check if partial orders are allowed and try without optionals.
				continue;
			}

			return ControlFlow::Ok(new_payload);
		}
	}
}

pub struct AppendManyOrders<P: Platform>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	_pool: OrderPool<P>,
}

impl<P: Platform> AppendManyOrders<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self {
			_pool: pool.clone(),
		}
	}
}

impl<P: Platform> Step<P> for AppendManyOrders<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Ok(payload)
	}
}
