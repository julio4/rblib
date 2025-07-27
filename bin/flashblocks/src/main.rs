use {
	crate::args::{Cli, CliExt},
	rblib::{
		pool::{AppendOneOrder, OrderPool},
		prelude::*,
		reth::optimism::node::{OpAddOns, OpNode},
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

			let pipeline = Pipeline::<FlashBlocks>::default()
				.with_prologue(OptimismPrologue)
				.with_pipeline(
					Loop,
					(
						AppendOneOrder::from_pool(&pool),
						OrderByPriorityFee,
						RemoveRevertedTransactions,
					),
				)
				.with_epilogue(BuilderEpilogue);

			let opnode = OpNode::new(cli_args.rollup_args.clone());

			let handle = builder
				.with_types::<OpNode>()
				.with_components(opnode.components().payload(pipeline.into_service()))
				.with_add_ons(OpAddOns::default())
				.extend_rpc_modules(move |mut rpc_ctx| pool.configure_rpc(&mut rpc_ctx))
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();

	Ok(())
}
