use {
	crate::{
		args::OpRbuilderArgs,
		bundle::FlashBlocksBundle,
		rpc::{BundleRpcApi, BundlesRpcApiServer},
	},
	rblib::{
		reth::{
			optimism::node::{
				OpAddOns,
				OpConsensusBuilder,
				OpEngineApiBuilder,
				OpEngineValidatorBuilder,
				OpExecutorBuilder,
				OpNetworkBuilder,
				OpNode,
				OpPoolBuilder,
			},
			providers::providers::BlockchainProvider,
		},
		*,
	},
	reth_db_api::{Database, database_metrics::DatabaseMetrics},
	reth_node_builder::{
		FullNodeTypesAdapter,
		NodeAdapter,
		NodeBuilder,
		NodeBuilderWithComponents,
		NodeComponentsBuilder,
		NodeTypesWithDBAdapter,
		WithLaunchContext,
		components::{ComponentsBuilder, PayloadServiceBuilder, PoolBuilder},
	},
	reth_optimism_rpc::OpEthApiBuilder,
	std::sync::Arc,
};

/// Defines the FlashBlocks platform.
///
/// This platform is mainly fully derived from the Optimism platform with few
/// modifications, such as:
/// - Custom bundle type that is used to represent the FlashBlocks bundles.
#[derive(Debug, Clone, Default)]
pub struct FlashBlocks;

impl Platform for FlashBlocks {
	type Bundle = FlashBlocksBundle;
	type DefaultLimits = types::DefaultLimits<Optimism>;
	type EvmConfig = types::EvmConfig<Optimism>;
	type NodeTypes = types::NodeTypes<Optimism>;
	type PooledTransaction = types::PooledTransaction<Optimism>;

	fn evm_config<P>(chainspec: Arc<types::ChainSpec<P>>) -> Self::EvmConfig
	where
		P: traits::PlatformExecBounds<Self>,
	{
		Optimism::evm_config::<Self>(chainspec)
	}

	fn next_block_environment_context<P>(
		chainspec: &types::ChainSpec<P>,
		parent: &types::Header<P>,
		attributes: &types::PayloadBuilderAttributes<P>,
	) -> types::NextBlockEnvContext<P>
	where
		P: traits::PlatformExecBounds<Self>,
	{
		Optimism::next_block_environment_context::<Self>(
			chainspec, parent, attributes,
		)
	}

	fn build_payload<P, Provider>(
		payload: Checkpoint<P>,
		provider: &Provider,
	) -> Result<types::BuiltPayload<P>, PayloadBuilderError>
	where
		P: traits::PlatformExecBounds<Self>,
		Provider: traits::ProviderBounds<Self>,
	{
		Optimism::build_payload::<P, Provider>(payload, provider)
	}
}

impl FlashBlocks {
	pub fn build_node<DB>(
		builder: WithLaunchContext<NodeBuilder<DB, types::ChainSpec<Self>>>,
		cli_args: OpRbuilderArgs,
	) -> ConfiguredNode<
		DB,
		impl PayloadServiceBuilder<
			FBFullNodeTypes<DB>,
			<OpPoolBuilder as PoolBuilder<FBFullNodeTypes<DB>>>::Pool,
			types::EvmConfig<FlashBlocks>,
		>,
	>
	where
		DB: Database + DatabaseMetrics + Clone + Unpin + 'static,
	{
		let pipeline = Pipeline::<Self>::default();
		let opnode = OpNode::new(cli_args.rollup_args.clone());

		builder
			.with_types::<OpNode>()
			.with_components(opnode.components().payload(pipeline.into_service()))
			.with_add_ons(OpAddOns::default())
			.extend_rpc_modules(move |rpc_ctx| {
				rpc_ctx
					.modules
					.add_or_replace_configured(BundleRpcApi.into_rpc())?;
				Ok(())
			})
	}
}

type Provider<DB> =
	BlockchainProvider<NodeTypesWithDBAdapter<types::NodeTypes<FlashBlocks>, DB>>;

type FBFullNodeTypes<DB> =
	FullNodeTypesAdapter<types::NodeTypes<FlashBlocks>, DB, Provider<DB>>;

type FBComponentsBuilder<DB, PB> = ComponentsBuilder<
	FBFullNodeTypes<DB>,
	OpPoolBuilder,
	PB,
	OpNetworkBuilder,
	OpExecutorBuilder,
	OpConsensusBuilder,
>;

type FBAddOns<DB, C> = OpAddOns<
	NodeAdapter<FBFullNodeTypes<DB>, C>,
	OpEthApiBuilder,
	OpEngineValidatorBuilder,
	OpEngineApiBuilder<OpEngineValidatorBuilder>,
>;

type ConfiguredNode<DB, PB> = WithLaunchContext<
	NodeBuilderWithComponents<
		FBFullNodeTypes<DB>,
		FBComponentsBuilder<DB, PB>,
		FBAddOns<
			DB,
			<FBComponentsBuilder<DB, PB> as NodeComponentsBuilder<
				FBFullNodeTypes<DB>,
			>>::Components,
		>,
	>,
>;
