//! Order pool maintenance utilities.
//!
//! This is not a public API, but rather a set of utilities that are used
//! internally by the order pool to maintain its state and react to events.

use {super::*, std::collections::HashSet};

impl<P: Platform> OrderPool<P> {
	/// Removes orders that became permanently ineligible after the given block
	/// header became the tip of the chain.
	pub(super) fn remove_invalidated_orders(
		&self,
		header: &SealedHeader<types::Header<P>>,
	) {
		let ineligible = self
			.inner
			.orders
			.iter()
			.filter_map(|order| {
				if let Order::Bundle(bundle) = order.value()
					&& bundle.is_permanently_ineligible(header)
				{
					Some(*order.key())
				} else {
					None
				}
			})
			.collect::<HashSet<_>>();

		self
			.inner
			.orders
			.retain(|order_hash, _| !ineligible.contains(order_hash));
	}
}
