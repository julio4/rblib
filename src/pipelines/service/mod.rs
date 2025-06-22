//! This module implements the machinery that takes a declarative pipeline
//! definition and turns it into a Reth compatible payload builder.

use {
	crate::*,
	alloc::{boxed::Box, sync::Arc},
	job::JobGenerator,
	reth::{
		api::NodeTypes,
		builder::{components::PayloadServiceBuilder, BuilderContext},
		providers::CanonStateSubscriptions,
	},
	reth_payload_builder::{PayloadBuilderHandle, PayloadBuilderService},
	tracing::debug,
};

mod job;

pub(crate) struct PipelineServiceBuilder<P: Platform> {
	platform: P,
	pipeline: Pipeline,
}

impl<P: Platform> PipelineServiceBuilder<P> {
	pub fn new(pipeline: Pipeline, platform: P) -> Self {
		Self { pipeline, platform }
	}
}

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
		let pipeline = self.pipeline;
		debug!("Spawning payload builder: {pipeline:#?}");

		let service = ServiceContext {
			pool,
			provider: ctx.provider().clone(),
			evm_config: Plat::evm_config(ctx.chain_spec()),
			chain_spec: ctx.chain_spec().clone(),
			platform: self.platform,
		};

		let (service, builder) = PayloadBuilderService::new(
			JobGenerator::new(pipeline, service),
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
	platform: Plat,
}

impl<Plat, Provider, Pool> ServiceContext<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pub const fn provider(&self) -> &Provider {
		&self.provider
	}

	pub const fn pool(&self) -> &Pool {
		&self.pool
	}

	pub const fn evm_config(&self) -> &Plat::EvmConfig {
		&self.evm_config
	}

	pub const fn chain_spec(&self) -> &Arc<types::ChainSpec<Plat>> {
		&self.chain_spec
	}

	pub const fn platform(&self) -> &Plat {
		&self.platform
	}
}

pub mod traits {
	use {
		crate::*,
		reth::{
			api::FullNodeTypes,
			providers::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory},
		},
		reth_evm::ConfigureEvm,
		reth_transaction_pool::{PoolTransaction, TransactionPool},
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

	pub trait PoolBounds<P: Platform>:
		TransactionPool<
			Transaction: PoolTransaction<Consensus = types::Transaction<P>>,
		> + Unpin
		+ 'static
	{
	}

	impl<T, P: Platform> PoolBounds<P> for T where
		T: TransactionPool<
				Transaction: PoolTransaction<Consensus = types::Transaction<P>>,
			> + Unpin
			+ 'static
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
