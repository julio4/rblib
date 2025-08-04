use {
	crate::args::{Cli, CliExt, OpRbuilderArgs},
	rblib::{
		pool::*,
		prelude::*,
		reth::optimism::node::{
			OpEngineApiBuilder,
			OpEngineValidatorBuilder,
			OpNode,
		},
		steps::*,
	},
};

mod args;
mod bundle;
mod platform;

#[cfg(test)]
mod tests;

pub use platform::FlashBlocks;

fn main() -> eyre::Result<()> {
	Cli::parsed()
		.run(|builder, cli_args| async move {
			let pool = OrderPool::<FlashBlocks>::default();
			let pipeline = build_pipeline(&cli_args, &pool);
			let opnode = OpNode::new(cli_args.rollup_args.clone());

			let handle = builder
				.with_types::<OpNode>()
				.with_components(
					opnode
						.components()
						.attach_pool(&pool)
						.payload(pipeline.into_service()),
				)
				.with_add_ons(
					opnode
						.add_ons_builder::<types::RpcTypes<FlashBlocks>>()
						.build::<_, OpEngineValidatorBuilder, OpEngineApiBuilder<OpEngineValidatorBuilder>>(),
				)
				.extend_rpc_modules(move |mut rpc_ctx| pool.attach_rpc(&mut rpc_ctx))
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();

	Ok(())
}

pub fn build_pipeline(
	cli_args: &OpRbuilderArgs,
	pool: &OrderPool<FlashBlocks>,
) -> Pipeline<FlashBlocks> {
	let mut pipeline = if cli_args.revert_protection {
		Pipeline::<FlashBlocks>::default()
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(
					AppendOneOrder::from_pool(pool),
					OrderByPriorityFee::default(),
					RemoveRevertedTransactions,
				),
			)
	} else {
		Pipeline::<FlashBlocks>::default()
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(
					AppendOneOrder::from_pool(pool),
					OrderByPriorityFee::default(),
				),
			)
	};

	if let Some(ref signer) = cli_args.builder_signer {
		let epilogue = BuilderEpilogue::with_signer(signer.clone().into());
		let limiter = epilogue.limiter();
		pipeline = pipeline.with_epilogue(epilogue).with_limits(limiter);
	}

	pipeline
}
