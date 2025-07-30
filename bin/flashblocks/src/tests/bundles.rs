use {
	crate::{FlashBlocks, bundle::FlashBlocksBundle, tests::transfer_tx_compact},
	jsonrpsee::core::ClientError,
	rblib::{
		alloy::{
			consensus::Transaction,
			eips::Typed2718,
			optimism::consensus::DEPOSIT_TX_TYPE_ID,
			primitives::U256,
		},
		pool::{BundleResult, BundlesApiClient},
		prelude::*,
		test_utils::*,
	},
	tracing::debug,
};

#[tokio::test]
async fn one_valid_tx_included() -> eyre::Result<()> {
	let node = FlashBlocks::test_node().await?;

	let bundle_with_one_tx =
		FlashBlocksBundle::with_transactions(vec![transfer_tx_compact(
			0, 0, 1_000_000,
		)]);
	let bundle_hash = bundle_with_one_tx.hash();

	let result = BundlesApiClient::<FlashBlocks>::send_bundle(
		&node.rpc_client().await?,
		bundle_with_one_tx,
	)
	.await?;

	assert_eq!(result, BundleResult { bundle_hash });

	let block = node.next_block().await?;
	debug!("Built block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + 1 bundle tx
	assert_eq!(block.tx(1).unwrap().value(), U256::from(1_000_000));

	Ok(())
}

#[tokio::test]
async fn two_valid_txs_included() -> eyre::Result<()> {
	let node = FlashBlocks::test_node().await?;

	let bundle_with_two_txs = FlashBlocksBundle::with_transactions(vec![
		transfer_tx_compact(0, 0, 1_000_000),
		transfer_tx_compact(0, 1, 2_000_000),
	]);
	let bundle_hash = bundle_with_two_txs.hash();

	let result = BundlesApiClient::<FlashBlocks>::send_bundle(
		&node.rpc_client().await?,
		bundle_with_two_txs,
	)
	.await?;

	assert_eq!(result, BundleResult { bundle_hash });

	let block = node.next_block().await?;
	debug!("Built block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 3); // sequencer deposit tx + 2 bundle txs
	assert_eq!(block.tx(1).unwrap().value(), U256::from(1_000_000));
	assert_eq!(block.tx(2).unwrap().value(), U256::from(2_000_000));

	Ok(())
}

/// Ensures that a a user transaction send to the RPC interface of the node
/// makes its way into the next block.
#[tokio::test]
async fn non_bundle_tx_included_in_block() -> eyre::Result<()> {
	// builders signer is not configured, so won't produce a builder tx
	let node = FlashBlocks::test_node().await?;

	let txhash = *node
		.send_tx(node.build_tx().transfer().value(U256::from(1_234_000)))
		.await
		.expect("transaction should be sent successfully")
		.tx_hash();

	let block = node.next_block().await?;
	debug!("produced block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + 1 user tx
	assert!(
		block.includes(&txhash),
		"Block should include the transaction"
	);

	let transactions = block.transactions.into_transactions_vec();

	let sequencer_tx = transactions.first().unwrap();
	assert_eq!(sequencer_tx.ty(), DEPOSIT_TX_TYPE_ID);

	let user_tx = transactions.last().unwrap();
	assert_eq!(user_tx.value(), U256::from(1_234_000));

	Ok(())
}

#[tokio::test]
async fn min_block_number_constraint() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn max_block_number_constraint() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn min_block_timestamp_constraint() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn max_block_timestamp_constraint() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn empty_bundle_rejected_by_rpc() -> eyre::Result<()> {
	let node = FlashBlocks::test_node().await?;

	let empty_bundle = FlashBlocksBundle::with_transactions(vec![]);
	let result = BundlesApiClient::<FlashBlocks>::send_bundle(
		&node.rpc_client().await?,
		empty_bundle,
	)
	.await;

	assert!(
		result.is_err(),
		"Expected error for empty bundle, got {result:?}"
	);

	let Err(ClientError::Call(error)) = result else {
		panic!("Expected Call error, got {result:?}");
	};

	assert_eq!(
		error.code(),
		jsonrpsee::types::ErrorCode::InvalidParams.code()
	);

	assert_eq!(
		error.message(),
		"bundle must contain at least one transaction"
	);

	Ok(())
}
