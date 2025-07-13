use {
	super::framework::*,
	crate::{steps::*, *},
	alloy::{network::TransactionBuilder, primitives::U256},
	tracing::info,
};

#[tokio::test]
async fn empty_payload_when_all_txs_revert_loop() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(
			AppendOneTransactionFromPool::default(),
			PriorityFeeOrdering,
			RevertProtection,
		),
	);

	let node = Ethereum::create_test_node(pipeline).await.unwrap();

	let mut reverts = vec![];
	for i in 0..10 {
		let tx = node.build_tx().reverting().with_value(U256::from(3000 + i));
		reverts.push(*node.send_tx(tx).await.unwrap().tx_hash());
	}

	let block = node.next_block().await.unwrap();
	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn transfers_included_reverts_excluded_loop() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(
			AppendOneTransactionFromPool::default(),
			PriorityFeeOrdering,
			RevertProtection,
		),
	);

	let node = Ethereum::create_test_node(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		let tx = node.build_tx().transfer().with_value(U256::from(i));
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
		!block.includes(&reverts),
		"Block should not include any reverts"
	);

	// non-reverting transactions
	assert_eq!(block.transactions.len(), transfers.len());
}

#[tokio::test]
async fn transfers_included_reverts_excluded_flat() {
	let pipeline = Pipeline::default()
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	let node = Ethereum::create_test_node(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		let tx = node.build_tx().transfer().with_value(U256::from(i));
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
		!block.includes(&reverts),
		"Block should not include any reverts"
	);

	// non-reverting transactions
	assert_eq!(block.transactions.len(), transfers.len());
}
