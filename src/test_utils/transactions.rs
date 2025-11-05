use {
	crate::{alloy, prelude, reth, test_utils},
	alloy::{
		consensus::SignableTransaction,
		network::{TransactionBuilder, TxSignerSync},
		primitives::{Address, U256},
		signers::local::PrivateKeySigner,
	},
	prelude::*,
	reth::{ethereum::primitives::SignedTransaction, primitives::Recovered},
	test_utils::FundedAccounts,
};

/// A mock tx to easily get a valid tx from `FundedAccounts` with `key` and
/// `nonce`
pub fn mock_tx<P: PlatformWithRpcTypes>(
	key: u32,
	nonce: u64,
) -> Recovered<types::Transaction<P>> {
	transfer_tx::<P>(&FundedAccounts::signer(key), nonce, U256::from(10u64))
}

#[allow(clippy::missing_panics_doc)]
pub fn transfer_tx<P: PlatformWithRpcTypes>(
	signer: &PrivateKeySigner,
	nonce: u64,
	value: U256,
) -> Recovered<types::Transaction<P>> {
	let mut tx = types::TransactionRequest::<P>::default()
		.with_nonce(nonce)
		.with_to(Address::random())
		.with_value(value)
		.with_gas_price(1_000_000_000)
		.with_gas_limit(21_000)
		.with_max_priority_fee_per_gas(1_000_000)
		.with_max_fee_per_gas(2_000_000)
		.build_unsigned()
		.expect("valid transaction request");

	let sig = signer
		.sign_transaction_sync(&mut tx)
		.expect("signing should succeed");

	let signed_tx: types::TxEnvelope<P> = tx.into_signed(sig).into();
	let signed_tx: types::Transaction<P> = signed_tx.into();
	signed_tx.with_signer(signer.address())
}
