use {
	crate::{FlashBlocks, args::OpRbuilderArgs},
	rblib::{
		alloy::{
			consensus::Transaction,
			network::TransactionResponse,
			primitives::{Address, U256},
		},
		test_utils::{BlockResponseExt, FundedAccounts},
	},
	tracing::debug,
};

/// This is a smoke test that ensures that transactions are included in blocks
/// and that the block generator is functioning correctly.
#[tokio::test]
async fn chain_produces_blocks() -> eyre::Result<()> {
	todo!()
}

/// Ensure that the chain produces blocks with a builder transaction
/// when the builder signer is provided in the CLI arguments.
#[tokio::test]
async fn chain_produces_blocks_with_builder_tx() -> eyre::Result<()> {
	let node = FlashBlocks::test_node_with_cli_args(OpRbuilderArgs {
		builder_signer: Some(FundedAccounts::signer(0).into()),
		..Default::default()
	})
	.await?;

	let block = node.next_block().await?;
	debug!("produced block: {block:#?}");

	assert_eq!(block.header.number, 1);
	assert_eq!(block.tx_count(), 2); // sequencer deposit tx + builder tx

	let builder_tx = block.tx(1).unwrap();
	assert_eq!(builder_tx.nonce(), 0);
	assert_eq!(builder_tx.value(), U256::ZERO);
	assert_eq!(builder_tx.to(), Some(Address::ZERO));
	assert_eq!(builder_tx.input(), "flashbots rblib block #1".as_bytes());
	assert_eq!(builder_tx.from(), FundedAccounts::signer(0).address());

	Ok(())
}
