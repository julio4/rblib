use {
	super::framework::*,
	crate::{steps::*, *},
	alloy::{
		consensus::BlockHeader,
		network::{BlockResponse, TransactionBuilder, TransactionResponse},
		primitives::U256,
	},
	op_alloy::consensus::DEPOSIT_TX_TYPE_ID,
	tracing::info,
};

#[rblib_test(Ethereum, Optimism)]
async fn empty_pipeline_builds_empty_payload<P: TestablePlatform>() {
	let empty_pipeline = Pipeline::default();
	let node = P::create_test_node(empty_pipeline).await.unwrap();

	for _ in 0..10 {
		let _ = node.send_tx(node.build_tx().transfer()).await.unwrap();
	}

	let block = node.next_block().await.unwrap();

	// ensure we've built the first block past genesis
	assert_eq!(block.header().number(), 1);

	if_platform!(Ethereum => {
		// Ethereum should not include any transactions
		assert!(block.is_empty(), "Block should be empty");
	});

	if_platform!(Optimism => {
		// Optimism should only include a sequencer deposit transaction
		assert_eq!(block.tx_count(), 1,
			"Optimism block should have only one sequencer transaction");

		assert_eq!(
			block.tx(0).unwrap().transaction_type().unwrap(),
			DEPOSIT_TX_TYPE_ID,
			"Optimism sequencer transaction should be a deposit tx"
		);
	});
}

#[rblib_test(Ethereum, Optimism)]
async fn pipeline_with_no_txs_builds_empty_payload<P: TestablePlatform>() {
	let pipeline = Pipeline::default()
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	let node = P::create_test_node(pipeline).await.unwrap();
	let block = node.next_block().await.unwrap();
	assert_eq!(block.header().number(), 1);

	if_platform!(Ethereum => {
		// Ethereum should not include any transactions
		assert!(block.is_empty(), "Block should be empty");
	});

	if_platform!(Optimism => {
		// Optimism should only include a sequencer deposit transaction
		assert_eq!(block.tx_count(), 1,
			"Optimism block should have only one sequencer transaction");

		assert_eq!(
			block.tx(0).unwrap().transaction_type().unwrap(),
			DEPOSIT_TX_TYPE_ID,
			"Optimism sequencer transaction should be a deposit tx"
		);
	});
}

#[rblib_test(Ethereum, Optimism)]
async fn all_transactions_included_ethereum<P: TestablePlatform>() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(AppendOneTransactionFromPool::default(), PriorityFeeOrdering),
	);

	let node = P::create_test_node(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		let tx = node.build_tx().transfer().with_value(U256::from(i + 1));
		transfers.push(*node.send_tx(tx).await.unwrap().tx_hash());
	}

	let mut reverts = vec![];
	for i in 0..4 {
		let tx = node.build_tx().reverting().with_value(U256::from(3000 + i));
		reverts.push(*node.send_tx(tx).await.unwrap().tx_hash());
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

	assert_eq!(block.header().number(), 1);

	if_platform!(Ethereum => {
		assert_eq!(block.tx_count(), transfers.len() + reverts.len());
	});

	if_platform!(Optimism => {
		// Optimism should include an extra sequencer deposit transaction
		assert_eq!(block.tx_count(), transfers.len() + reverts.len() + 1);
	});
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
