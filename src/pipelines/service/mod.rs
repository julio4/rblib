//! This module implements the machinery that takes a declarative pipeline
//! definition and turns it into a Reth compatible payload builder.

use {
	crate::*,
	alloc::{boxed::Box, sync::Arc},
	core::marker::PhantomData,
	job::JobGenerator,
	reth::{
		builder::{components::PayloadServiceBuilder, BuilderContext},
		providers::CanonStateSubscriptions,
	},
	reth_payload_builder::{PayloadBuilderHandle, PayloadBuilderService},
	tracing::debug,
};

mod job;

pub(crate) struct PipelineServiceBuilder<P: Platform>(
	pub Pipeline,
	pub PhantomData<P>,
);

impl<Plat, Node, Pool, EvmConfig> PayloadServiceBuilder<Node, Pool, EvmConfig>
	for PipelineServiceBuilder<Plat>
where
	Plat: Platform,
	Node: traits::NodeBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
	EvmConfig: traits::EvmConfigBounds<Plat>,
{
	async fn spawn_payload_builder_service(
		self,
		ctx: &BuilderContext<Node>,
		pool: Pool,
		_evm_config: EvmConfig,
	) -> eyre::Result<PayloadBuilderHandle<<Node::Types as NodeTypes>::Payload>>
	{
		let pipeline = self.0;
		debug!("Spawning payload builder: {pipeline:#?}");

		let service = Arc::new(ServiceContext {
			pool,
			provider: ctx.provider().clone(),
			evm_config: Plat::evm_config(ctx.chain_spec()),
			chain_spec: ctx.chain_spec().clone(),
		});

		let (service, builder) = PayloadBuilderService::new(
			JobGenerator::new(Arc::new(pipeline), service),
			ctx.provider().canonical_state_stream(),
		);

		ctx
			.task_executor()
			.spawn_critical("payload_builder_service", Box::pin(service));

		Ok(builder)
	}
}

pub struct ServiceContext<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pool: Pool,
	provider: Provider,
	evm_config: Plat::EvmConfig,
	chain_spec: Arc<types::ChainSpec<Plat>>,
}

impl<Plat, Provider, Pool> ServiceContext<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pub fn provider(&self) -> &Provider {
		&self.provider
	}

	pub fn pool(&self) -> &Pool {
		&self.pool
	}

	pub fn evm_config(&self) -> &Plat::EvmConfig {
		&self.evm_config
	}

	pub fn chain_spec(&self) -> &Arc<types::ChainSpec<Plat>> {
		&self.chain_spec
	}
}

pub mod traits {
	use {
		crate::*,
		reth::{
			api::FullNodeTypes,
			providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory},
		},
		reth_transaction_pool::TransactionPool,
	};

	pub trait NodeBounds<P: Platform>:
		FullNodeTypes<Types = P::NodeTypes>
	{
	}

	impl<T, P: Platform> NodeBounds<P> for T where
		T: FullNodeTypes<Types = P::NodeTypes>
	{
	}

	pub trait ProviderBounds<P: Platform>:
		StateProviderFactory
		+ ChainSpecProvider<ChainSpec = types::ChainSpec<P>>
		+ BlockReaderIdExt<Header = types::Header<P>>
		+ Clone
	{
	}

	impl<T, P: Platform> ProviderBounds<P> for T where
		T: StateProviderFactory
			+ ChainSpecProvider<ChainSpec = types::ChainSpec<P>>
			+ BlockReaderIdExt<Header = types::Header<P>>
			+ Clone
	{
	}

	pub trait PoolBounds<P: Platform>: TransactionPool + Unpin + 'static {}

	impl<T, P: Platform> PoolBounds<P> for T where
		T: TransactionPool + Unpin + 'static
	{
	}

	pub trait EvmConfigBounds<P: Platform>:
		ConfigureEvm<Primitives = types::Primitives<P>> + Send + Sync
	{
	}

	impl<T, P: Platform> EvmConfigBounds<P> for T where
		T: ConfigureEvm<Primitives = types::Primitives<P>> + Send + Sync
	{
	}
}
