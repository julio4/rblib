use {
	crate::{
		args::{Cli, CliExt, OpRbuilderArgs},
		rpc::TransactionStatusRpc,
	},
	core::time::Duration,
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
mod rpc;

#[cfg(test)]
mod tests;

pub use platform::FlashBlocks;

fn main() {
	Cli::parsed()
		.run(|builder, cli_args| async move {
			let pool = OrderPool::<FlashBlocks>::default();
			let pipeline = build_pipeline(&cli_args, &pool);
			let opnode = OpNode::new(cli_args.rollup_args.clone());
			let tx_status_rpc = TransactionStatusRpc::new(&pipeline);

			#[expect(clippy::large_futures)]
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
				.extend_rpc_modules(move |mut rpc_ctx| {
					pool.attach_rpc(&mut rpc_ctx)?;
					tx_status_rpc.attach_rpc(&mut rpc_ctx)?;
					Ok(())
				})
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();
}

pub fn build_pipeline(
	cli_args: &OpRbuilderArgs,
	pool: &OrderPool<FlashBlocks>,
) -> Pipeline<FlashBlocks> {
	let mut pipeline = if cli_args.revert_protection {
		Pipeline::<FlashBlocks>::named("standard")
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(
					AppendOrders::from_pool(pool),
					OrderByPriorityFee::default(),
					RemoveRevertedTransactions::default(),
				)
					.with_limits(
						Scaled::new().deadline(Minus(Duration::from_millis(500))),
					),
			)
	} else {
		Pipeline::<FlashBlocks>::named("standard")
			.with_prologue(OptimismPrologue)
			.with_pipeline(
				Loop,
				(AppendOrders::from_pool(pool), OrderByPriorityFee::default()),
			)
	};

	if let Some(ref signer) = cli_args.builder_signer {
		let epilogue = BuilderEpilogue::with_signer(signer.clone().into());
		let limiter = epilogue.limiter();
		pipeline = pipeline.with_epilogue(epilogue).with_limits(limiter);
	}

	pool.attach_pipeline(&pipeline);

	pipeline
}
