use {
	crate::args::{Cli, CliExt},
	rblib::{
		reth::optimism::node::{OpAddOns, OpNode},
		steps::*,
		*,
	},
};

mod args;
mod rpc;

#[tokio::main]
async fn main() -> eyre::Result<()> {
	// pipeline for the standard (non-flashblocks) optimism block builder
	let standard = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(
			Loop,
			(
				AppendOneTransactionFromPool::default(),
				PriorityFeeOrdering,
				RevertProtection,
			),
		);

	Cli::parsed()
		.run(|builder, args| async move {
			let handle = builder
				.with_types::<OpNode>()
				.with_components(
					OpNode::new(args.rollup_args)
						.components()
						.payload(standard.into_service()),
				)
				.with_add_ons(OpAddOns::default())
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();

	Ok(())
}

// pub struct RBuilderPlatform;

// impl Platform for RBuilderPlatform {
// 	type DefaultLimits = <Optimism as Platform>::DefaultLimits;
// 	type EvmConfig = <Optimism as Platform>::EvmConfig;
// 	type NodeTypes = <Optimism as Platform>::NodeTypes;
// 	type PooledTransaction = <Optimism as Platform>::PooledTransaction;

// 	fn evm_config(
// 		chainspec: std::sync::Arc<types::ChainSpec<Self>>,
// 	) -> Self::EvmConfig {
// 		Optimism::evm_config(chainspec)
// 	}

// 	fn next_block_environment_context(
// 		chainspec: &types::ChainSpec<Self>,
// 		parent: &types::Header<Self>,
// 		attributes: &types::PayloadBuilderAttributes<Self>,
// 	) -> types::NextBlockEnvContext<Self> {
// 		Optimism::next_block_environment_context(chainspec, parent, attributes)
// 	}

// 	fn construct_payload<Pool, Provider>(
// 		block: &BlockContext<Self>,
// 		transactions: Vec<reth::primitives::Recovered<types::Transaction<Self>>>,
// 		transaction_pool: &Pool,
// 		provider: &Provider,
// 	) -> Result<
// 		types::BuiltPayload<Self>,
// 		reth::payload::builder::PayloadBuilderError,
// 	>
// 	where
// 		Pool: traits::PoolBounds<Self>,
// 		Provider: traits::ProviderBounds<Self>,
// 	{
// 		Optimism::construct_payload(block, transactions, transaction_pool,
// provider) 	}
// }
