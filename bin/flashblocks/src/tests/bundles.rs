use {
	crate::{
		FlashBlocks,
		bundle::{BundleResult, FlashBlocksBundle},
		rpc::BundlesRpcApiClient,
		tests::transfer_tx_compact,
	},
	rblib::{
		alloy::{
			consensus::Transaction,
			primitives::{B256, U256},
		},
		test_utils::*,
		*,
	},
};

#[tokio::test]
async fn bundle_with_one_tx_is_included() -> eyre::Result<()> {
	let node = FlashBlocks::create_test_node(Pipeline::default()).await?;

	let bundle_with_one_tx =
		FlashBlocksBundle::with_transactions(vec![transfer_tx_compact(
			0, 0, 1_000_000,
		)]);
	let bundle_hash = bundle_with_one_tx.hash();

	let result = BundlesRpcApiClient::send_bundle(
		&node.rpc_client().await?,
		bundle_with_one_tx,
	)
	.await?;

	assert_eq!(result, BundleResult { bundle_hash });

	let block = node.next_block().await?;

	assert_eq!(block.header.number, 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + 1 bundle tx
	assert_eq!(block.tx(1).unwrap().value(), U256::from(1_000_000));

	Ok(())
}
