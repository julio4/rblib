//! Order pool status reporting
//!
//! Methods and types in this module are used to report the status of existing
//! orders execution and other information that help the pool decide about what
//! to do with the orders in the pool.

use {
	super::*,
	reth::{node::builder::BlockBody, primitives::SealedBlock},
	tracing::trace,
};

impl<P: Platform> OrderPool<P> {
	/// Invoked when an order was proposed by the pool but it failed to create a
	/// checkpoint because of an execution error.
	///
	/// Here we will need to decide whether the execution error permanently
	/// invalidates the order and it should be removed from the pool, or if it
	/// can be retried later.
	#[expect(clippy::needless_pass_by_value)] // <-- todo remove this when implemented
	pub fn report_execution_error(
		&self,
		order_hash: B256,
		error: ExecutionError<P>,
	) {
		trace!("order marked as invalid: {order_hash} - {error:?}");

		match error {
			// This order is permanently ineligible for inclusion in this and any
			// future payloads. remove it permanently from the pool so it
			// won't be attempted again
			ExecutionError::IneligibleBundle(Eligibility::PermanentlyIneligible) => {
				self.remove(&order_hash);
			}

			ExecutionError::InvalidSignature(_) => {
				// This order is permanently ineligible for inclusion in this and any
				// future payloads. remove it permanently from the pool so it
				// won't be attempted again
				self.remove(&order_hash);
			}

			// TODO: Implement this logic
			_ => {}
		}
	}

	/// Invoked when an order was proposed by the pool through thr `best_orders()`
	/// and there was an attempt to include it in a payload.
	pub fn report_inclusion_attempt(
		&self,
		_order_hash: B256,
		_block: &BlockContext<P>,
	) {
	}

	/// Signals to the order pool that a block has been committed to the chain.
	/// This will remove all orders that had any of their transactions included
	/// in the payload.
	pub fn report_committed_block(&self, block: &SealedBlock<types::Block<P>>) {
		for tx in block.body().transactions() {
			self.remove_any_with(*tx.tx_hash());
		}
	}
}
