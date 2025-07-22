//! Minimal example of Payload API for Ethereum
//!
//! In this example we will build a block with two transactions on top of the
//! genesis block of the Sepolia testnet. The transactions will transfer some
//! ETH between two random accounts.
//!
//! We're also demonstrating how to create a checkpoint that gets discarded
//! from the final payload, but can be used to simulate the state of the
//! payload at any point in time.

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
	let initial_checkpoint = block_context.start();
	println!("checkpoint created: {initial_checkpoint}");

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

	// Checkpoints can be applied on top of each other, creating a progressive
	// history of state changes.
	let second_checkpoint = initial_checkpoint.apply(tx1)?;
	println!("checkpoint created: {second_checkpoint}");

	let third_checkpoint = second_checkpoint.apply(tx2)?;
	println!("checkpoint created: {third_checkpoint}");

	// checkpoints can be built on top of any arbitrary checkpoint, they may be
	// cheaply discarded from the final payload. Many forks may coexist in the
	// payload building process.
	let discarded_checkpoint = second_checkpoint.apply(tx3)?;
	println!("checkpoint created: {discarded_checkpoint} (will be discarded)");

	// You can pick whatever checkpoint as the final payload and use it to
	// assemble a full block.
	let built_payload =
		Ethereum::build_payload(third_checkpoint, &state_provider)
			.expect("payload should be built successfully");

	println!("{built_payload:#?}");
	assert_eq!(built_payload.block().header().number, 1);
	assert_eq!(built_payload.block().header().parent_hash, parent.hash());
	assert_eq!(built_payload.block().body().transactions.len(), 2);

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
