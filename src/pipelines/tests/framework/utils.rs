use {
	super::{TransactionBuilder, FUNDED_PRIVATE_KEYS},
	crate::pipelines::tests::{Signer, ONE_ETH},
	alloy::{
		consensus::{EthereumTxEnvelope, TxEip4844},
		hex,
		primitives::{Address, BlockHash, TxHash},
		rpc::types::{Block, BlockTransactionHashes, Transaction},
	},
	reth_ethereum::primitives::SignerRecoverable,
	secp256k1::rand,
};

pub trait TransactionBuilderExt {
	fn random_valid_transfer(self) -> Self;
	fn random_reverting_transaction(self) -> Self;
}

impl TransactionBuilderExt for TransactionBuilder {
	fn random_valid_transfer(self) -> Self {
		self.with_to(Address::random()).with_value(1)
	}

	fn random_reverting_transaction(self) -> Self {
		// PUSH1 0x00 PUSH1 0x00 REVERT
		self.with_create().with_input(hex!("60006000fd").into())
	}
}

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
