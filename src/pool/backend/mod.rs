use {
	super::{
		Order,
		rpc::{BundleRpcApi, BundlesApiServer},
	},
	crate::{alloy, prelude::*, reth},
	alloy::primitives::{B256, TxHash},
	dashmap::{DashMap, DashSet},
	reth::{
		ethereum::primitives::SignedTransaction,
		node::builder::{FullNodeComponents, rpc::RpcContext},
		rpc::api::eth::EthApiTypes,
	},
	std::sync::{Arc, OnceLock},
};

mod select;
mod setup;
mod status;

pub use setup::ComponentBuilderPoolInstaller;

/// Implements an order pool that handles mempool operations for transactions
/// and bundles.
///
/// Notes:
///  - This type is cheap to clone, all clones of this type share the same
///    underlying instance.
///  - This type is referenced by steps and RPC modules when constructing a
///    pipeline and Reth node.
#[derive(Debug)]
pub struct OrderPool<P: Platform> {
	inner: Arc<OrderPoolInner<P>>,
}

/// Orders manipulation
impl<P: Platform> OrderPool<P> {
	/// Adds a new order to the pool and makes it potentially available to be
	/// returned by `best_orders()`.
	pub fn insert(&self, order: Order<P>) {
		let order_hash = order.hash();

		for tx in order.transactions() {
			// keep track of all orders that contain this transaction.
			// When this transaction ends up in a produced payload, all orders
			// containing it will be invalidated and removed from the pool.
			let txhash = *tx.tx_hash();
			self
				.inner
				.txmap
				.entry(txhash)
				.or_default()
				.insert(order_hash);
		}

		self.inner.orders.insert(order_hash, order);
	}

	/// Removes an order and makes it no longer available through `best_orders()`.
	pub fn remove(&self, order_hash: &B256) -> Option<Order<P>> {
		self.inner.orders.remove(order_hash).map(|(_, order)| order)
	}

	/// Removes all orders that contain the a specific transaction hash.
	pub fn remove_all_containing(&self, txhash: TxHash) {
		if let Some((_, orders)) = self.inner.txmap.remove(&txhash) {
			for order_hash in orders {
				self.inner.orders.remove(&order_hash);
			}
		}
	}
}

impl<P: Platform> Clone for OrderPool<P> {
	fn clone(&self) -> Self {
		Self {
			inner: Arc::clone(&self.inner),
		}
	}
}

impl<P: Platform> Default for OrderPool<P> {
	fn default() -> Self {
		Self {
			inner: Arc::new(OrderPoolInner::default()),
		}
	}
}

#[derive(Debug, Default)]
struct OrderPoolInner<P: Platform> {
	orders: DashMap<B256, Order<P>>,
	txmap: DashMap<TxHash, DashSet<B256>>,
	system_pool: OnceLock<Arc<dyn core::any::Any + Send + Sync>>,
}
