use {
	crate::args::{Cli, CliExt},
	rblib::{
		reth::optimism::node::{OpAddOns, OpNode},
		steps::*,
		*,
	},
};

mod args;

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
