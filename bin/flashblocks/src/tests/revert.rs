use {
	crate::{FlashBlocks, tests::*},
	tracing::debug,
};

#[tokio::test]
async fn bundle_with_one_reverted_tx_not_included() -> eyre::Result<()> {
	let node = FlashBlocks::test_node().await?;

	let bundle_without_reverts = random_valid_bundle(1);
	let bundle_with_reverts = random_bundle_with_reverts(0, 1);

	BundlesApiClient::<FlashBlocks>::send_bundle(
		&node.rpc_client().await?,
		bundle_without_reverts.clone(),
	)
	.await?;

	BundlesApiClient::<FlashBlocks>::send_bundle(
		&node.rpc_client().await?,
		bundle_with_reverts.clone(),
	)
	.await?;

	let block = node.next_block().await?;
	debug!("Built block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + 1 valid bundle tx

	assert_has_sequencer_tx!(&block);

	let tx = block.tx(1).unwrap();
	assert_eq!(
		tx.tx_hash(),
		bundle_without_reverts.transactions()[0].tx_hash()
	);
	Ok(())
}

#[tokio::test]
async fn when_disabled_reverted_txs_are_included() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn bundle_with_one_reverting_tx_allowed_to_revert_included()
-> eyre::Result<()> {
	todo!()
}

/// If a transaction reverts and gets dropped it, the eth_getTransactionReceipt
/// should return an error message that it was dropped.
#[tokio::test]
async fn reverted_dropped_tx_has_valid_receipt_status() -> eyre::Result<()> {
	todo!()
}
