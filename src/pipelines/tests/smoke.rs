use {
	super::{framework::*, steps::*},
	crate::*,
	alloy::{
		primitives::{address, U256},
		providers::Provider,
	},
	tracing::info,
};

#[tokio::test]
async fn one_tx_included_in_one_block() {
	let pipeline = Pipeline::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let tx = node
		.new_transaction()
		.with_value(100_000_000u128)
		.with_to(address!("0xF0109fC8DF283027b6285cc889F5aA624EaC1F55"))
		.send()
		.await
		.unwrap();

	info!("Transaction sent: {tx:#?}");

	let block = node.build_new_block().await.unwrap();

	info!("Block built: {block:#?}");
}
