use {
	alloy::{
		consensus::{EthereumTxEnvelope, TxEip4844},
		network::{TransactionBuilder, TxSignerSync},
		primitives::{Address, B256, U256},
		rpc::types::engine::PayloadAttributes,
		signers::local::PrivateKeySigner,
	},
	rblib::*,
	reth::{
		chainspec::{EthChainSpec, SEPOLIA},
		ethereum::{TransactionSigned, primitives::SignedTransaction},
		payload::builder::EthPayloadBuilderAttributes,
		primitives::Recovered,
		providers::test_utils::{ExtendedAccount, MockEthProvider},
		rpc::types::TransactionRequest,
	},
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
	let chainspec = SEPOLIA.clone();
	let parent = chainspec.genesis_header.clone();
	let signers = [PrivateKeySigner::random(), PrivateKeySigner::random()];
	let state_provider =
		MockEthProvider::default().with_chain_spec(chainspec.as_ref().clone());

	// prefund signers with 1 ETH each
	state_provider.extend_accounts(signers.iter().map(|s| {
		(
			s.address(),
			ExtendedAccount::new(0, U256::from(100_000_000_000_000_000u64)),
		)
	}));

	let sp2 = state_provider.clone();

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
		Box::new(state_provider),
		chainspec,
	)?;

	let first_checkpoint = block_context.start();
	println!("initial checkpoint created: {first_checkpoint:#?}");

	let tx1 = make_transfer_tx(
		&signers[0],
		0,
		signers[1].address(),
		U256::from(50_000_000u64),
	);

	let second_checkpoint = first_checkpoint.apply(tx1)?;
	println!("second checkpoint created: {second_checkpoint:#?}");

	let tx2 = make_transfer_tx(
		&signers[1],
		0,
		signers[0].address(),
		U256::from(25_000_000u64),
	);

	let third_checkpoint = second_checkpoint.apply(tx2)?;
	println!("third checkpoint created: {third_checkpoint:#?}");

	let block = Ethereum::construct_payload(third_checkpoint, &sp2)
		.expect("payload should be built successfully");

	println!("built block: {block:#?}");

	Ok(())
}

fn make_transfer_tx(
	signer: &PrivateKeySigner,
	nonce: u64,
	to: Address,
	value: U256,
) -> Recovered<EthereumTxEnvelope<TxEip4844>> {
	let mut tx = TransactionRequest::default()
		.with_nonce(nonce)
		.with_chain_id(SEPOLIA.chain_id())
		.with_to(to)
		.value(value)
		.with_gas_price(1_000_000_000)
		.with_gas_limit(21_000)
		.with_max_priority_fee_per_gas(1_000_000)
		.with_max_fee_per_gas(2_000_000)
		.build_unsigned()
		.expect("valid transaction request");

	let sig = signer
		.sign_transaction_sync(&mut tx)
		.expect("signing should succeed");

	TransactionSigned::new_unhashed(tx.into(), sig).with_signer(signer.address())
}
