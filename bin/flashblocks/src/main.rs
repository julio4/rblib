use {
	crate::args::{Cli, CliExt, OpRbuilderArgs},
	rblib::{
		pool::{AppendOneOrder, OrderPool},
		prelude::*,
		reth::{builder::Node, optimism::node::OpNode},
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
				.with_components(opnode.components().payload(pipeline.into_service()))
				.with_add_ons(opnode.add_ons())
				.extend_rpc_modules(move |mut rpc_ctx| pool.configure_rpc(&mut rpc_ctx))
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
	let mut pipeline = if cli_args.enable_revert_protection {
		Pipeline::<FlashBlocks>::default()
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(
					AppendOneOrder::from_pool(pool),
					OrderByPriorityFee,
					RemoveRevertedTransactions,
				),
			)
	} else {
		Pipeline::<FlashBlocks>::default()
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(AppendOneOrder::from_pool(pool), OrderByPriorityFee),
			)
	};

	if let Some(ref signer) = cli_args.builder_signer {
		let epilogue = BuilderEpilogue::with_signer(signer.clone().into());
		let limiter = epilogue.limiter();
		pipeline = pipeline.with_epilogue(epilogue).with_limits(limiter);
	}

	pipeline
}
