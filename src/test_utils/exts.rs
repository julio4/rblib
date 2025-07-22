use {
	super::*,
	crate::*,
	alloy::{
		consensus::{SignableTransaction, Signed},
		hex,
		network::{
			BlockResponse,
			Network,
			TransactionBuilder,
			TransactionResponse,
			TxSignerSync,
		},
		primitives::{Address, B256, Bytes, TxHash, TxKind, U256},
		signers::Signature,
	},
	reth::{
		ethereum::node::engine::EthPayloadAttributes as PayloadAttributes,
		payload::builder::EthPayloadBuilderAttributes,
		primitives::SealedHeader,
	},
};

pub trait BlockResponseExt<T: TransactionResponse> {
	fn tx(&self, index: usize) -> Option<&T>;
	fn includes(&self, txs: &impl AsTxs) -> bool;
	fn tx_count(&self) -> usize;
	fn is_empty(&self) -> bool {
		self.tx_count() == 0
	}
}

impl<T: TransactionResponse, B> BlockResponseExt<T> for B
where
	T: TransactionResponse,
	B: BlockResponse<Transaction = T>,
{
	fn includes(&self, txs: &impl AsTxs) -> bool {
		txs.as_txs().into_iter().all(|txhash| {
			self
				.transactions()
				.txns()
				.any(|included| included.tx_hash() == txhash)
		})
	}

	fn tx_count(&self) -> usize {
		self.transactions().txns().count()
	}

	fn is_empty(&self) -> bool {
		self.transactions().is_empty()
	}

	fn tx(&self, index: usize) -> Option<&T> {
		self.transactions().txns().nth(index)
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
	#[must_use]
	fn with_funded_signer(self, key: u32) -> Self;

	/// Use a random funded account that is defined in genesis of the test
	/// local node.
	#[must_use]
	fn with_random_funded_signer(self) -> Self;

	/// Use a specific funded account that is defined in genesis of the test
	/// local node at index 0.
	#[must_use]
	fn with_default_signer(self) -> Self;

	/// Use a random non-zero priority fee for the transaction.
	/// The priority fee is a random value between 1 and 100000.
	#[must_use]
	fn with_random_priority_fee(self) -> Self;

	/// Creates a transaction that will always revert.
	/// It will be a `CREATE` transaction with the input
	/// `PUSH1 0x00 PUSH1 0x00 REVERT`.
	#[must_use]
	fn reverting(self) -> Self;

	/// Creates a transaction that is a valid transfer of a small amount of ether.
	#[must_use]
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

pub trait PayloadBuilderAttributesExt {
	fn mock_for_parent(parent: &SealedHeader) -> Self;
}

impl PayloadBuilderAttributesExt for EthPayloadBuilderAttributes {
	fn mock_for_parent(parent: &SealedHeader) -> Self {
		EthPayloadBuilderAttributes::new(parent.hash(), PayloadAttributes {
			timestamp: parent.header().timestamp + 1,
			prev_randao: B256::random(),
			suggested_fee_recipient: Address::random(),
			withdrawals: None,
			parent_beacon_block_root: None,
		})
	}
}

#[cfg(feature = "optimism")]
impl PayloadBuilderAttributesExt for reth::optimism::node::OpPayloadAttributes {
	fn mock_for_parent(parent: &SealedHeader) -> Self {
		use reth::optimism::{
			chainspec::constants::{
				BASE_MAINNET_MAX_GAS_LIMIT,
				TX_SET_L1_BLOCK_OP_MAINNET_BLOCK_124665056,
			},
			node::OpPayloadAttributes,
		};

		OpPayloadAttributes {
			payload_attributes: PayloadAttributes {
				timestamp: parent.header().timestamp + 1,
				parent_beacon_block_root: Some(B256::ZERO),
				withdrawals: Some(vec![]),
				..Default::default()
			},
			transactions: Some(vec![
				TX_SET_L1_BLOCK_OP_MAINNET_BLOCK_124665056.into(),
			]),
			gas_limit: Some(BASE_MAINNET_MAX_GAS_LIMIT),
			..Default::default()
		}
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
				unimplemented!("Step `{}` is not implemented", stringify!($name))
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
				unimplemented!("Step `{}` is not implemented", stringify!($name))
			}
		}
	};
}
