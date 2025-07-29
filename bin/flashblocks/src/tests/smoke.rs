use {
	crate::{FlashBlocks, args::OpRbuilderArgs},
	rblib::{
		alloy::{
			consensus::Transaction,
			primitives::{Address, U256},
		},
		prelude::Pipeline,
		test_utils::{BlockResponseExt, FundedAccounts, TestNodeFactory},
	},
	tracing::debug,
};

/// This is a smoke test that ensures that transactions are included in blocks
/// and that the block generator is functioning correctly.
#[tokio::test]
async fn chain_produces_blocks() -> eyre::Result<()> {
	todo!()
}

/// This is a smoke test that ensures that the builder transaction is included
/// in the block as the last transaction.
#[tokio::test]
async fn chain_produces_blocks_with_builder_tx() -> eyre::Result<()> {
	let signer = FundedAccounts::signer(0);
	let args = OpRbuilderArgs {
		builder_signer: Some(signer.into()),
		..Default::default()
	};

	let node = FlashBlocks::create_test_node_with_args(
		Pipeline::<FlashBlocks>::default(),
		args,
	)
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

	Ok(())
}
