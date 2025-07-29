use {
	crate::{FlashBlocks, args::OpRbuilderArgs},
	rblib::{
		alloy::{
			consensus::Transaction,
			eips::Typed2718,
			network::TransactionResponse,
			optimism::consensus::DEPOSIT_TX_TYPE_ID,
			primitives::{Address, U256},
		},
		test_utils::{BlockResponseExt, FundedAccounts, TransactionRequestExt},
	},
	tracing::debug,
};

/// This is a smoke test that ensures that sequencer transactions are included
/// in blocks and that the block generator is functioning correctly.
#[tokio::test]
async fn chain_produces_blocks() -> eyre::Result<()> {
	// builders signer is not configured, so won't produce a builder tx
	let node = FlashBlocks::test_node().await?;

	let block = node.next_block().await?;
	debug!("produced block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 1); // sequencer deposit tx

	let sequencer_tx = block.tx(0).unwrap();
	assert_eq!(sequencer_tx.ty(), DEPOSIT_TX_TYPE_ID);

	Ok(())
}

/// Ensures that a a user transaction send to the RPC interface of the node
/// makes its way into the next block.
#[tokio::test]
async fn one_tx_included_in_block() -> eyre::Result<()> {
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

/// Ensure that the chain produces blocks with a builder transaction
/// when the builder signer is provided in the CLI arguments.
#[tokio::test]
async fn blocks_have_builder_tx() -> eyre::Result<()> {
	let node = FlashBlocks::test_node_with_cli_args(OpRbuilderArgs {
		builder_signer: Some(FundedAccounts::signer(0).into()),
		..Default::default()
	})
	.await?;

	let block = node.next_block().await?;
	debug!("produced block: {block:#?}");

	assert_eq!(block.number(), 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + builder tx

	let builder_tx = block.tx(1).unwrap();
	assert_eq!(builder_tx.nonce(), 0);
	assert_eq!(builder_tx.value(), U256::ZERO);
	assert_eq!(builder_tx.to(), Some(Address::ZERO));
	assert_eq!(builder_tx.input(), "flashbots rblib block #1".as_bytes());
	assert_eq!(builder_tx.from(), FundedAccounts::signer(0).address());

	Ok(())
}
