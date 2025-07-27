use {
	super::{Order, OrderPool},
	crate::{
		alloy::primitives::B256,
		prelude::*,
		reth::ethereum::primitives::SignedTransaction,
	},
	dashmap::DashSet,
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
}

impl<P: Platform> AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self {
			pool: pool.clone(),
			attempted: DashSet::new(),
		}
	}
}

impl<P: Platform> Step<P> for AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	/// Clear the list of previously added transactions before the we begin
	/// building for a new payload.
	async fn before_job(
		self: Arc<Self>,
		_: Arc<StepContext<P>>,
	) -> Result<(), PayloadBuilderError> {
		self.attempted.clear();
		Ok(())
	}

	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let mut orders = self.pool.best_orders();
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
				if deadline <= Instant::now() {
					// stop the loop and return the current payload
					debug!(
						"Payload building deadline reached for {}",
						ctx.block().payload_id()
					);
					return ControlFlow::Break(payload);
				}
			}

			let Some(order) = orders.next() else {
				// No more orders in the pool to add to the payload.
				return ControlFlow::Break(payload);
			};

			let order_hash = order.hash();
			if self.attempted.contains(&order_hash) {
				// This order was already attempted to be added to the payload.
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
				// this order is not eligible for adding to the payload
				continue;
			}

			let Ok(executable): Result<Executable<P>, _> = (match order {
				Order::Transaction(tx) => tx.try_into_executable(),
				Order::Bundle(bundle) => bundle.try_into_executable(),
			}) else {
				continue;
			};

			// we could potentially fit this order into the payload,
			// create a new state checkpoint with the order applied and check if we
			// can still fit within the gas limit.
			let Ok(new_payload) = payload.apply(executable) else {
				// skip this order, it cannot be applied to the payload
				continue;
			};

			if new_payload.cumulative_gas_used() > ctx.limits().gas_limit {
				// We've exceeded the gas limit with this order.
				// TODO: check if this order would potentially fit if some optional
				// transactions were removed from the payload.
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
	pool: OrderPool<P>,
}

impl<P: Platform> AppendManyOrders<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self { pool: pool.clone() }
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
