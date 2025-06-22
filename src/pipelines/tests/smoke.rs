use {
	super::{framework::*, steps::*},
	crate::*,
	alloy::{
		primitives::{address, U256},
		providers::Provider,
	},
	reth::api::Block,
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

	assert!(
		block
			.body()
			.transactions()
			.any(|tx_in_block| tx_in_block.hash() == tx.tx_hash()),
		"Transaction should be included in the block"
	);
}

#[tokio::test]
#[ignore]
async fn reth_minimal_integration_example() {
	use {
		reth::cli::Cli,
		reth_ethereum::node::{node::EthereumAddOns, EthereumNode},
	};

	let pipeline = Pipeline::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	Cli::parse_args()
		.run(|builder, _| async move {
			let handle = builder
				.with_types::<EthereumNode>()
				.with_components(
					EthereumNode::components()
						.payload(pipeline.into_service(EthereumMainnet)),
				)
				.with_add_ons(EthereumAddOns::default())
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();
}
