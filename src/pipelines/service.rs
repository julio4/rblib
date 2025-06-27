//! This module implements the machinery that takes a declarative pipeline
//! definition and turns it into a Reth compatible payload builder.

use {
	crate::{pipelines::job::PayloadJob, *},
	reth::{
		api::{NodeTypes, PayloadBuilderAttributes},
		builder::{components::PayloadServiceBuilder, BuilderContext, NodeConfig},
		providers::CanonStateSubscriptions,
	},
	reth_payload_builder::{
		PayloadBuilderHandle,
		PayloadBuilderService,
		PayloadJob as RethPayloadJobTrait,
		*,
	},
	std::sync::Arc,
	tracing::debug,
};

/// This type is the bridge between Reth's payload builder API and the
/// pipelines API. It will take a pipeline instance and turn it into what
/// eventually becomes a Paylod Job Generator. They payload Job Generator
/// will be responsible for creating new [`PayloadJob`] instances
/// whenever a new payload request comes in from the CL Node.
pub(crate) struct PipelineServiceBuilder<P: Platform> {
	pipeline: Pipeline<P>,
}

impl<P: Platform> PipelineServiceBuilder<P> {
	pub fn new(pipeline: Pipeline<P>) -> Self {
		Self { pipeline }
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
		debug!("Spawning payload builder service for: {pipeline:#?}");

		let service = ServiceContext {
			pool,
			provider: ctx.provider().clone(),
			evm_config: Plat::evm_config(ctx.chain_spec()),
			node_config: ctx.config().clone(),
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

/// There is one service context instance per reth node. This type gives
/// individual jobs access to the node state, transaction pool and other
/// runtime facilities that are managed by reth.
pub struct ServiceContext<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pool: Pool,
	provider: Provider,
	evm_config: Plat::EvmConfig,
	node_config: NodeConfig<types::ChainSpec<Plat>>,
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

	pub const fn node_config(&self) -> &NodeConfig<types::ChainSpec<Plat>> {
		&self.node_config
	}

	pub const fn chain_spec(&self) -> &Arc<types::ChainSpec<Plat>> {
		&self.node_config().chain
	}
}

/// This type is stored inside the [`PayloadBuilderService`] type in Reth.
/// There's one instance of this type per node and it is instantiated during the
/// node startup inside `spawn_payload_builder_service`.
///
/// The responsibility of this type is to respond to new payload requests when
/// FCU calls come from the CL Node. Each FCU call will generate a new PayloadID
/// on its side and will pass it to the `new_payload_job` method.
pub struct JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pipeline: Arc<Pipeline<Plat>>,
	service: Arc<ServiceContext<Plat, Provider, Pool>>,
}

impl<Plat, Provider, Pool> JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pub fn new(
		pipeline: Pipeline<Plat>,
		service: ServiceContext<Plat, Provider, Pool>,
	) -> Self {
		let pipeline = Arc::new(pipeline);
		let service = Arc::new(service);

		Self { pipeline, service }
	}
}

impl<Plat, Provider, Pool> PayloadJobGenerator
	for JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	type Job = PayloadJob<Plat, Provider, Pool>;

	fn new_payload_job(
		&self,
		attribs: <Self::Job as RethPayloadJobTrait>::PayloadAttributes,
	) -> Result<Self::Job, PayloadBuilderError> {
		debug!("PayloadJobGenerator::new_payload_job {attribs:#?}");
		let header = if attribs.parent().is_zero() {
			self.service.provider().latest_header()?.ok_or_else(|| {
				PayloadBuilderError::MissingParentHeader(attribs.parent())
			})
		} else {
			self
				.service
				.provider()
				.sealed_header_by_hash(attribs.parent())?
				.ok_or_else(|| {
					PayloadBuilderError::MissingParentHeader(attribs.parent())
				})
		}?;

		let base_state =
			self.service.provider().state_by_block_hash(header.hash())?;

		// This is the beginning of the state manipulation API usage from within
		// the pipelines API.
		let block_ctx = BlockContext::new(
			header,
			attribs,
			base_state,
			self.service.evm_config().clone(),
			self.service.chain_spec().clone(),
		)
		.map_err(PayloadBuilderError::other)?;

		let pipeline = Arc::clone(&self.pipeline);
		let service_ctx = Arc::clone(&self.service);

		Ok(PayloadJob::new(pipeline, block_ctx, service_ctx))
	}
}
