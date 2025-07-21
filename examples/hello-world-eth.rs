use {
	alloy::{
		network::{TransactionBuilder, TxSignerSync},
		primitives::{Address, B256, U256},
		rpc::types::engine::PayloadAttributes,
		signers::local::PrivateKeySigner,
	},
	rblib::*,
	reth::{
		chainspec::{EthChainSpec, SEPOLIA},
		payload::builder::EthPayloadBuilderAttributes,
		providers::test_utils::MockEthProvider,
		rpc::types::TransactionRequest,
	},
	reth_ethereum::{TransactionSigned, primitives::SignedTransaction},
	reth_origin::providers::test_utils::ExtendedAccount,
	std::sync::Arc,
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
	let chainspec = SEPOLIA.clone();
	let parent = chainspec.genesis_header.clone();
	let signers = [PrivateKeySigner::random(), PrivateKeySigner::random()];
	let state_provider = Box::new(
		MockEthProvider::default().with_chain_spec(Arc::clone(&chainspec)),
	);

	// prefund signers with 1 ETH each
	state_provider.extend_accounts(signers.iter().map(|s| {
		(
			s.address(),
			ExtendedAccount::new(0, U256::from(100_000_000_000_000_000u64)),
		)
	}));

	let payload_attribs =
		EthPayloadBuilderAttributes::new(parent.hash(), PayloadAttributes {
			timestamp: parent.header().timestamp + 1,
			prev_randao: B256::random(),
			suggested_fee_recipient: Address::random(),
			withdrawals: None,
			parent_beacon_block_root: None,
		});

	let block_context = BlockContext::<Ethereum>::new(
		parent,
		payload_attribs,
		state_provider,
		chainspec,
	)?;

	let first_checkpoint = block_context.start();
	println!("initial checkpoint created: {first_checkpoint:#?}");

	let mut tx1 = TransactionRequest::default()
		.with_nonce(0)
		.with_chain_id(block_context.chainspec().chain_id())
		.with_to(Address::random())
		.value(U256::from(1000))
		.with_gas_price(1_000_000_000)
		.with_gas_limit(21_000)
		.with_max_priority_fee_per_gas(1_000_000)
		.with_max_fee_per_gas(2_000_000)
		.build_unsigned()?;
	let sig = signers[0].sign_transaction_sync(&mut tx1)?;
	let tx1 = TransactionSigned::new_unhashed(tx1.into(), sig)
		.with_signer(signers[0].address());

	let second_checkpoint = first_checkpoint.apply(tx1)?;
	println!("second checkpoint created: {second_checkpoint:#?}");

	Ok(())
}
