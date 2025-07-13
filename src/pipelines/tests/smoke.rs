use {
	super::framework::*,
	crate::{steps::*, *},
	alloy::{network::TransactionBuilder, primitives::U256},
	tracing::info,
};

#[tokio::test]
async fn empty_pipeline_builds_empty_payload() {
	let empty_pipeline = Pipeline::default();
	let node = Ethereum::create_test_node(empty_pipeline).await.unwrap();

	for _ in 0..10 {
		let _ = node.send_tx(node.build_tx().transfer()).await.unwrap();
	}
	let block = node.next_block().await.unwrap();

	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn pipeline_with_no_txs_builds_empty_payload() {
	let pipeline = Pipeline::default()
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	let node = Ethereum::create_test_node(pipeline).await.unwrap();
	let block = node.next_block().await.unwrap();

	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn all_transactions_included() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(AppendOneTransactionFromPool::default(), PriorityFeeOrdering),
	);

	let node = Ethereum::create_test_node(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		let tx = node
			.build_tx()
			.transfer()
			.with_value(U256::from(i))
			.with_random_priority_fee();
		transfers.push(*node.send_tx(tx).await.unwrap().tx_hash());
	}

	let mut reverts = vec![];
	for i in 0..4 {
		reverts.push(
			*node
				.send_tx(
					node
						.build_tx()
						.reverting()
						.with_value(U256::from(3000 + i))
						.with_random_priority_fee(),
				)
				.await
				.unwrap()
				.tx_hash(),
		);
	}

	let block = node.next_block().await.unwrap();

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

	let pipeline = Pipeline::<Ethereum>::default()
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
