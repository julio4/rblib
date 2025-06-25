use {
	super::{framework::*, steps::*},
	crate::*,
	tracing::info,
};

#[tokio::test]
async fn empty_pipeline_builds_empty_payload() {
	let empty_pipeline = Pipeline::default();
	let node = LocalNode::ethereum(empty_pipeline).await.unwrap();
	for _ in 0..10 {
		let _ = node
			.new_transaction()
			.random_valid_transfer()
			.send()
			.await
			.unwrap();
	}
	let block = node.build_new_block().await.unwrap();

	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn one_tx_included_in_one_block() {
	let pipeline = Pipeline::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let mut transfers = vec![];
	for _ in 0..10 {
		transfers.push(
			*node
				.new_transaction()
				.random_valid_transfer()
				.send()
				.await
				.unwrap()
				.tx_hash(),
		);
	}

	let mut reverts = vec![];
	for _ in 0..4 {
		reverts.push(
			*node
				.new_transaction()
				.random_reverting_transaction()
				.send()
				.await
				.unwrap()
				.tx_hash(),
		);
	}

	let block = node.build_new_block().await.unwrap();

	info!("Block built: {block:#?}");

	assert!(
		block.includes(&transfers),
		"Block should include all valid transfers"
	);

	assert!(
		!block.includes(&reverts),
		"Block should not include any reverts"
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
