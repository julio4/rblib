use {
	crate::pipelines::tests::FundedAccounts,
	alloy::{
		consensus::{SignableTransaction, Signed},
		hex,
		network::{Network, TransactionBuilder, TxSignerSync},
		primitives::{Address, Bytes, TxHash, TxKind, U256},
		rpc::types::{Block, BlockTransactionHashes, Transaction},
		signers::Signature,
	},
};

pub trait BlockTransactionsExt {
	fn includes(&self, txs: &impl AsTxs) -> bool;
}

impl BlockTransactionsExt for Block<Transaction> {
	fn includes(&self, txs: &impl AsTxs) -> bool {
		txs
			.as_txs()
			.into_iter()
			.all(|tx| self.transactions.hashes().any(|included| included == tx))
	}
}

impl BlockTransactionsExt for BlockTransactionHashes<'_, Transaction> {
	fn includes(&self, txs: &impl AsTxs) -> bool {
		let mut included_tx_iter = self.clone();
		txs
			.as_txs()
			.iter()
			.all(|tx| included_tx_iter.any(|included| included == *tx))
	}
}

pub trait AsTxs {
	fn as_txs(&self) -> Vec<TxHash>;
}

impl AsTxs for TxHash {
	fn as_txs(&self) -> Vec<TxHash> {
		vec![*self]
	}
}

impl AsTxs for Vec<TxHash> {
	fn as_txs(&self) -> Vec<TxHash> {
		self.clone()
	}
}

pub trait TransactionRequestExt<N: Network> {
	/// Use a specific funded account that is defined in genesis of the test
	/// local node.
	fn with_funded_signer(self, key: u32) -> Self;

	/// Use a random funded account that is defined in genesis of the test
	/// local node.
	fn with_random_funded_signer(self) -> Self;

	/// Use a specific funded account that is defined in genesis of the test
	/// local node at index 0.
	fn with_default_signer(self) -> Self;

	/// Use a random non-zero priority fee for the transaction.
	/// The priority fee is a random value between 1 and 100000.
	fn with_random_priority_fee(self) -> Self;

	/// Creates a transaction that will always revert.
	/// It will be a `CREATE` transaction with the input
	/// `PUSH1 0x00 PUSH1 0x00 REVERT`.
	fn reverting(self) -> Self;

	/// Creates a transaction that is a valid transfer of a small amount of ether.
	fn transfer(self) -> Self;

	fn build_with_known_signer(self) -> eyre::Result<N::TxEnvelope>
	where
		N::UnsignedTx: SignableTransaction<Signature>,
		N::TxEnvelope: From<Signed<N::UnsignedTx, Signature>>;
}

impl<T, N> TransactionRequestExt<N> for T
where
	T: TransactionBuilder<N>,
	N: Network,
{
	fn with_funded_signer(self, key: u32) -> Self {
		self.with_from(FundedAccounts::signer(key).address())
	}

	fn with_random_funded_signer(self) -> Self {
		self.with_from(FundedAccounts::random().address())
	}

	fn with_default_signer(self) -> Self {
		self.with_funded_signer(0)
	}

	fn with_random_priority_fee(self) -> Self {
		let max_priority_fee_per_gas = rand::random::<u128>() % 100_000 + 1;
		self.with_max_priority_fee_per_gas(max_priority_fee_per_gas)
	}

	fn reverting(self) -> Self {
		self
			.with_kind(TxKind::Create)
			// PUSH1 0x00 PUSH1 0x00 REVERT
			.with_input(hex!("60006000fd"))
	}

	fn transfer(self) -> Self {
		self
			.with_to(Address::random())
			.with_value(U256::from(1))
			.with_gas_limit(21000)
			.with_input(Bytes::new())
	}

	fn build_with_known_signer(self) -> eyre::Result<N::TxEnvelope>
	where
		N::UnsignedTx: SignableTransaction<Signature>,
		N::TxEnvelope: From<Signed<N::UnsignedTx, Signature>>,
	{
		let Some(from) = self.from() else {
			return Err(eyre::eyre!(
				"Transaction request must have a 'from' field, use sign_and_send_tx \
				 instead"
			));
		};

		let Some(signer) = FundedAccounts::by_address(from) else {
			return Err(eyre::eyre!("No known signer found for address: {from}"));
		};

		let mut tx = self.build_unsigned()?;
		let signature = signer.sign_transaction_sync(&mut tx)?;
		Ok(tx.into_signed(signature).into())
	}
}

#[macro_export]
macro_rules! make_step {
	($name:ident) => {
		#[derive(Debug, Clone)]
		pub struct $name;
		impl<P: Platform> Step<P> for $name {
			async fn step(
				self: std::sync::Arc<Self>,
				_: Checkpoint<P>,
				_: StepContext<P>,
			) -> ControlFlow<P> {
				todo!()
			}
		}
	};

	($name:ident, $state:ident) => {
		#[allow(dead_code)]
		#[derive(Debug, Clone)]
		pub struct $name($state);
		impl<P: Platform> Step<P> for $name {
			async fn step(
				self: std::sync::Arc<Self>,
				_: Checkpoint<P>,
				_: StepContext<P>,
			) -> ControlFlow<P> {
				todo!()
			}
		}
	};
}
