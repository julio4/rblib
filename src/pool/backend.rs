use {
	super::rpc::{BundleRpcApi, BundlesApiServer},
	crate::prelude::*,
	jsonrpsee::Methods,
	reth_node_builder::{FullNodeComponents, rpc::RpcContext},
	reth_rpc_api::eth::EthApiTypes,
	std::sync::Arc,
};

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
			inner: Arc::new(OrderPoolInner {
				_p: std::marker::PhantomData,
			}),
		}
	}
}

/// Node builder public api
impl<P: Platform> OrderPool<P> {
	pub fn configure_rpc<Node, EthApi>(
		&self,
		rpc_context: &mut RpcContext<Node, EthApi>,
	) -> eyre::Result<()>
	where
		Node: FullNodeComponents<Types = types::NodeTypes<P>>,
		EthApi: EthApiTypes,
	{
		rpc_context
			.modules
			.add_or_replace_configured(self.rpc_modules())?;

		Ok(())
	}

	pub fn rpc_modules(&self) -> impl Into<Methods> {
		BundleRpcApi::new(self).into_rpc()
	}
}

#[derive(Debug)]
struct OrderPoolInner<P: Platform> {
	_p: std::marker::PhantomData<P>,
}

impl<P: Platform> OrderPoolInner<P> {}
