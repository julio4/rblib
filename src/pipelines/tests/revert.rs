use {
	super::framework::*,
	crate::{steps::*, *},
	tracing::info,
};

#[tokio::test]
async fn empty_payload_when_all_txs_revert_loop() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(
			AppendNewTransactionFromPool::default(),
			PriorityFeeOrdering,
			RevertProtection,
		),
	);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let mut reverts = vec![];
	for i in 0..10 {
		reverts.push(
			*node
				.new_transaction()
				.random_reverting_transaction()
				.with_value(3000 + i)
				.send()
				.await
				.unwrap()
				.tx_hash(),
		);
	}

	let block = node.build_new_block().await.unwrap();
	assert_eq!(block.header.number, 1);
	assert!(block.transactions.is_empty(), "Block should be empty");
}

#[tokio::test]
async fn transfers_included_reverts_excluded_loop() {
	let pipeline = Pipeline::default().with_pipeline(
		Loop,
		(
			AppendNewTransactionFromPool::default(),
			PriorityFeeOrdering,
			RevertProtection,
		),
	);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		transfers.push(
			*node
				.new_transaction()
				.random_valid_transfer()
				.with_value(i)
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

	// non-reverting transactions
	assert_eq!(block.transactions.len(), transfers.len());
}

#[tokio::test]
async fn transfers_included_reverts_excluded_flat() {
	let pipeline = Pipeline::default()
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	let node = LocalNode::ethereum(pipeline).await.unwrap();

	let mut transfers = vec![];
	for i in 0..10 {
		transfers.push(
			*node
				.new_transaction()
				.random_valid_transfer()
				.with_value(i)
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

	// non-reverting transactions
	assert_eq!(block.transactions.len(), transfers.len());
}
