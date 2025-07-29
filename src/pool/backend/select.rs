use super::*;

/// Orders query and retrieval
impl<P: Platform> OrderPool<P> {
	pub fn best_orders(&self) -> impl Iterator<Item = Order<P>> + '_ {
		self.inner.orders.iter().map(|entry| entry.value().clone())
	}

	pub fn best_orders_for_block(
		&self,
		_block: &BlockContext<P>,
	) -> impl Iterator<Item = Order<P>> + '_ {
		self.best_orders()
	}
}
