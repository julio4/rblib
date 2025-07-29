//! Order pool setup and configuration functionality.
//!
//! Methods in this module are used to configure the host reth node and other
//! environment components to work with the order pool.

use super::*;

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
