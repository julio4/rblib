use {
	super::OrderPool,
	crate::prelude::*,
	serde::{Serialize, de::DeserializeOwned},
	std::{sync::Arc, time::Instant},
	tracing::debug,
};

pub struct AppendOneOrder<P: Platform>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pool: OrderPool<P>,
}

impl<P: Platform> AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	pub fn from_pool(pool: &OrderPool<P>) -> Self {
		Self { pool: pool.clone() }
	}
}

impl<P: Platform> Step<P> for AppendOneOrder<P>
where
	P::Bundle: Serialize + DeserializeOwned,
{
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
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

		ControlFlow::Ok(payload)
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
