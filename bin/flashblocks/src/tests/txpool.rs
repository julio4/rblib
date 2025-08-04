use {
	crate::FlashBlocks,
	rblib::{
		alloy::{
			consensus::Transaction,
			eips::Typed2718,
			optimism::consensus::DEPOSIT_TX_TYPE_ID,
			primitives::U256,
		},
		test_utils::*,
	},
	tracing::debug,
};

/// Ensure that user transactions send to the RPC interface of the node
/// that are not part of a bundle make their way into the block.
#[tokio::test]
async fn non_bundle_tx_included_in_block() -> eyre::Result<()> {
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
	assert!(block.includes(txhash));

	let transactions = block.transactions.into_transactions_vec();

	let sequencer_tx = transactions.first().unwrap();
	assert_eq!(sequencer_tx.ty(), DEPOSIT_TX_TYPE_ID);

	let user_tx = transactions.last().unwrap();
	assert_eq!(user_tx.value(), U256::from(1_234_000));

	Ok(())
}
