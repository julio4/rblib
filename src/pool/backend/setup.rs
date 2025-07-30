//! Order pool setup and configuration functionality.
//!
//! Methods in this module are used to configure the host reth node and other
//! environment components to work with the order pool.

use {
	super::*,
	reth_node_builder::{
		FullNodeTypes,
		NodeTypes,
		components::{ComponentsBuilder, PoolBuilder},
	},
	reth_transaction_pool::TransactionPool,
	tracing::debug,
};

impl<P: Platform> OrderPool<P> {
	/// Installs the order pool RPC endpoints for receiving bundles and other
	/// methods offered by the common reth rpc infra.
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
			.add_or_replace_configured(BundleRpcApi::new(self).into_rpc())?;

		Ok(())
	}
}

/// In the current implementation of the `OrderPool`, we use the default node
/// transaction pool for handling individual non-bundle transactions. This type
/// allows us to reuse the existing pool builder construction logic and store a
/// reference to the constructed pool in the `OrderPoolInner`.
///
/// In future iterations, we're expecting to have our own implementation of the
/// system transaction pool.
///
/// usage:
/// ```rust
/// Cli::parsed()
/// 		.run(|builder, cli_args| async move {
///     let pool = OrderPool::<FlashBlocks>::default();
/// 		let opnode = OpNode::new(cli_args);
///     let handle = builder
///         .with_types::<OpNode>()
///         .with_components(opnode.components().replace_pool(&pool))
///         .with_add_ons(opnode.add_ons())
///         .launch()
///         .await?;
///     handle.wait_for_node_exit().await
///     Ok(())
/// 		})
/// 		.unwrap();
/// ```
pub trait ComponentBuilderPoolInstaller<P: Platform> {
	type Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>;

	/// The type of the `ComponentsBuilder` with a wrapped pool builder.
	type Output<WrappedPoolB>;

	fn replace_pool(
		self,
		with: &OrderPool<P>,
	) -> Self::Output<
		impl PoolBuilder<
			Self::Node,
			Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
		> + use<Self, P>,
	>;
}

impl<P: Platform, Node, PoolB, PayloadB, NetworkB, ExecB, ConsB>
	ComponentBuilderPoolInstaller<P>
	for ComponentsBuilder<Node, PoolB, PayloadB, NetworkB, ExecB, ConsB>
where
	PoolB: PoolBuilderBounds<P, Node>,
	Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>,
{
	type Node = Node;
	type Output<WrappedPoolB> =
		ComponentsBuilder<Node, WrappedPoolB, PayloadB, NetworkB, ExecB, ConsB>;

	fn replace_pool(
		self,
		with: &OrderPool<P>,
	) -> Self::Output<
		impl PoolBuilder<
			Node,
			Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
		> + use<P, Node, PoolB, PayloadB, NetworkB, ExecB, ConsB>,
	> {
		let mut current_pool_builder = None;
		let current_components = self.map_pool(|pool| {
			current_pool_builder = Some(pool.clone());
			pool
		});

		current_components.pool(SystemPoolWrapper::new(
			current_pool_builder.unwrap(),
			with.inner.clone(),
		))
	}
}

/// This type wraps the system transaction pool builder and allows us to store a
/// reference to the built system pool in the `OrderPoolInner`. Reth node will
/// receive the pool built by the wrapped builder.
#[must_use]
struct SystemPoolWrapper<P: Platform, Builder> {
	builder: Builder,
	order_pool: Arc<OrderPoolInner<P>>,
}

impl<P: Platform, Builder> SystemPoolWrapper<P, Builder> {
	pub fn new<Node>(builder: Builder, order_pool: Arc<OrderPoolInner<P>>) -> Self
	where
		Builder: PoolBuilderBounds<P, Node>,
		Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>,
	{
		Self {
			builder,
			order_pool,
		}
	}
}

impl<P, Builder, Node> PoolBuilder<Node> for SystemPoolWrapper<P, Builder>
where
	P: Platform,
	Builder: PoolBuilderBounds<P, Node>,
	Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>,
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
		debug!("Order pool configured with system pool {built_pool:?}");
		Ok(built_pool)
	}
}

trait PoolBuilderBounds<P, Node>:
	PoolBuilder<
		Node,
		Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
	> + Clone
	+ Send
	+ Sync
where
	P: Platform,
	Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>,
{
}

impl<P: Platform, Node, T> PoolBuilderBounds<P, Node> for T
where
	P: Platform,
	T: PoolBuilder<
			Node,
			Pool: TransactionPool<Transaction = types::PooledTransaction<P>>,
		> + Clone
		+ Send
		+ Sync,
	Node: FullNodeTypes<Types: NodeTypes<Primitives = types::Primitives<P>>>,
{
}
