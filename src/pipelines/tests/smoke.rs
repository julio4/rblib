use {
	super::framework::*,
	crate::{steps::*, *},
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
async fn pipeline_with_no_txs_builds_empty_payload() {
	let pipeline = Pipeline::default()
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	let node = LocalNode::ethereum(pipeline).await.unwrap();
	let block = node.build_new_block().await.unwrap();

	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn all_transactions_included() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(AppendOneTransactionFromPool::default(), PriorityFeeOrdering),
	);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		transfers.push(
			*node
				.new_transaction()
				.random_valid_transfer()
				.with_value(i)
				.with_random_funded_account()
				.with_random_priority_fee()
				.send()
				.await
				.unwrap()
				.tx_hash(),
		);
	}

	let mut reverts = vec![];
	for i in 0..4 {
		reverts.push(
			*node
				.new_transaction()
				.random_reverting_transaction()
				.with_value(3000 + i)
				.with_random_priority_fee()
				.with_random_funded_account()
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
		block.includes(&reverts),
		"Block should not include any reverts"
	);

	assert_eq!(block.transactions.len(), transfers.len() + reverts.len());
}

#[tokio::test]
#[ignore = "This test never completes but we want to make sure that this \
            syntax compiles"]
async fn reth_minimal_integration_example() {
	use {
		reth::cli::Cli,
		reth_ethereum::node::{EthereumNode, node::EthereumAddOns},
	};

	let pipeline = Pipeline::<EthereumMainnet>::default()
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(
			Loop,
			(
				AppendOneTransactionFromPool::default(),
				PriorityFeeOrdering,
				TotalProfitOrdering,
				RevertProtection,
			),
		);

	Cli::parse_args()
		.run(|builder, _| async move {
			#[allow(clippy::large_futures)]
			let handle = builder
				.with_types::<EthereumNode>()
				.with_components(
					EthereumNode::components().payload(pipeline.into_service()),
				)
				.with_add_ons(EthereumAddOns::default())
				.launch()
				.await?;

			handle.wait_for_node_exit().await
		})
		.unwrap();
}
