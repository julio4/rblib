use {
	super::{
		steps::{
			BuilderEpilogue,
			GatherBestTransactions,
			PriorityFeeOrdering,
			RevertProtection,
			TotalProfitOrdering,
		},
		Pipeline,
	},
	crate::pipelines::tests::framework::LocalNode,
	reth::cli::Cli,
	reth_ethereum::node::{node::EthereumAddOns, EthereumNode},
};

#[tokio::test]
async fn one_tx_included_in_one_block() {
	let pipeline = Pipeline::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	let node = LocalNode::new(pipeline).await.unwrap();
}
