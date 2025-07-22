//! Minimal example of Payload API for Ethereum with bundles.
//!
//! In this example we will build a block with two transactions and one bundle
//! on top of the genesis block of the Sepolia testnet.

use {
	alloy::{
		consensus::{EthereumTxEnvelope, TxEip4844},
		network::{TransactionBuilder, TxSignerSync},
		primitives::{Address, U256},
		signers::local::PrivateKeySigner,
	},
	rblib::{
		test_utils::{
			FundedAccounts,
			PayloadBuilderAttributesExt,
			WithFundedAccounts,
		},
		*,
	},
	reth::{
		chainspec::{EthChainSpec, SEPOLIA},
		ethereum::{TransactionSigned, primitives::SignedTransaction},
		payload::builder::EthPayloadBuilderAttributes,
		primitives::Recovered,
		providers::test_utils::MockEthProvider,
		rpc::types::TransactionRequest,
	},
};

fn main() -> eyre::Result<()> {
	let chainspec = SEPOLIA.clone();
	let parent = chainspec.genesis_header.clone();
	let state_provider = MockEthProvider::default().with_funded_accounts();
	let payload_attribs = EthPayloadBuilderAttributes::mock_for_parent(&parent);

	// This is the entry point of the payload building API. We construct a
	// building context for a given block and attributes.
	let block_context = BlockContext::<Ethereum>::new(
		parent.clone(),
		payload_attribs,
		Box::new(state_provider.clone()),
		chainspec,
	)?;

	// Next we progressively build the payload by creating checkpoints that have
	// state mutations applied to them.
	let start = block_context.start();

	let tx1 = make_transfer_tx(
		&FundedAccounts::signer(0),
		0,
		Address::random(),
		U256::from(50_000_000u64),
	);

	let tx2 = make_transfer_tx(
		&FundedAccounts::signer(1),
		0,
		Address::random(),
		U256::from(25_000_000u64),
	);

	let tx3 = make_transfer_tx(
		&FundedAccounts::signer(0),
		1,
		Address::random(),
		U256::from(10_000_000u64),
	);

	let tx4 = make_transfer_tx(
		&FundedAccounts::signer(1),
		1,
		Address::random(),
		U256::from(5_000_000u64),
	);

	// A bundle is an atomic set of transactions that must be included together
	// consecutively in the same order as specified.
	let bundle = EthereumBundle::default()
		.with_transaction(tx2)
		.with_transaction(tx3);

	// Checkpoints can be applied on top of each other, creating a progressive
	// history of state changes.
	let payload = start.apply(tx1)?.apply(bundle)?.apply(tx4)?;
	let built_payload = Ethereum::build_payload(payload, &state_provider)
		.expect("payload should be built successfully");

	println!("{built_payload:#?}");
	assert_eq!(built_payload.block().header().number, 1);
	assert_eq!(built_payload.block().header().parent_hash, parent.hash());
	assert_eq!(built_payload.block().body().transactions.len(), 4);

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
