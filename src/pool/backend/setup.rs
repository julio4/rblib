//! Order pool setup and configuration functionality.
//!
//! Methods in this module are used to configure the host reth node and other
//! environment components to work with the order pool.

use {
	super::*,
	reth_node_builder::{FullNodeTypes, components::PoolBuilder},
	reth_optimism_node::OpPoolBuilder,
	reth_transaction_pool::TransactionPool,
};

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

impl<P> OrderPool<P>
where
	P: Platform<PooledTransaction = types::PooledTransaction<Optimism>>,
{
	pub fn component<Node>(
		&self,
	) -> impl PoolBuilder<
		Node,
		Pool: TransactionPool<Transaction = types::PooledTransaction<Optimism>>,
	> + use<Node, P>
	where
		Node: FullNodeTypes<Types = types::NodeTypes<P>>,
		OpPoolBuilder: PoolBuilder<
				Node,
				Pool: TransactionPool<Transaction = types::PooledTransaction<Optimism>>,
			>,
	{
		let builder =
			OpPoolBuilder::<types::PooledTransaction<Optimism>>::default();
		WrappedBuilder::new(builder, self.inner.clone())
	}
}

struct WrappedBuilder<P: Platform, Builder> {
	builder: Builder,
	order_pool: Arc<OrderPoolInner<P>>,
}

impl<P: Platform, Builder> WrappedBuilder<P, Builder>
where
	P: Platform,
{
	pub fn new<Node>(builder: Builder, order_pool: Arc<OrderPoolInner<P>>) -> Self
	where
		Builder: PoolBuilder<
				Node,
				Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
			>,
		Node: FullNodeTypes<Types = types::NodeTypes<P>>,
	{
		Self {
			builder,
			order_pool,
		}
	}
}

impl<P, Builder, Node> PoolBuilder<Node> for WrappedBuilder<P, Builder>
where
	P: Platform,
	Builder: PoolBuilder<
			Node,
			Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
		>,
	Node: FullNodeTypes<Types = types::NodeTypes<P>>,
{
	type Pool = Arc<Builder::Pool>;

	async fn build_pool(
		self,
		ctx: &reth_node_builder::BuilderContext<Node>,
	) -> eyre::Result<Self::Pool> {
		let built_pool = Arc::new(self.builder.build_pool(ctx).await?);
		self
			.order_pool
			.system_pool
			.set(built_pool.clone())
			.map_err(|_| eyre::eyre!("System pool already constructed"))?;
		tracing::info!("Order pool configured with system pool {built_pool:?}");
		Ok(built_pool)
	}
}
