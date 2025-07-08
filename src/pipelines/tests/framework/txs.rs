#![allow(dead_code)]

use {
	super::signer::Signer,
	crate::pipelines::tests::framework::FUNDED_PRIVATE_KEYS,
	alloy::{
		consensus::{SignableTransaction, TxEip1559, TxEnvelope},
		eips::{eip1559::MIN_PROTOCOL_BASE_FEE, BlockNumberOrTag, Encodable2718},
		hex,
		network::Ethereum,
		primitives::{Address, Bytes, TxKind, U256},
		providers::{PendingTransactionBuilder, Provider, RootProvider},
	},
	core::cmp::max,
	reth::primitives::Recovered,
	reth_ethereum::TransactionSigned,
};

#[derive(Clone)]
pub struct TransactionBuilder {
	provider: RootProvider,
	signer: Option<Signer>,
	nonce: Option<u64>,
	base_fee: Option<u128>,
	tx: TxEip1559,
}

impl TransactionBuilder {
	pub fn new(provider: RootProvider) -> Self {
		Self {
			provider,
			signer: None,
			nonce: None,
			base_fee: None,
			tx: TxEip1559 {
				chain_id: 1,
				gas_limit: 210000,
				..Default::default()
			},
		}
	}

	pub fn with_to(mut self, to: Address) -> Self {
		self.tx.to = TxKind::Call(to);
		self
	}

	pub fn with_create(mut self) -> Self {
		self.tx.to = TxKind::Create;
		self
	}

	pub fn with_funded_account(self, key: u64) -> Self {
		assert!(
			key < FUNDED_PRIVATE_KEYS.len() as u64,
			"Key index out of bounds, must be less than {}",
			FUNDED_PRIVATE_KEYS.len()
		);

		self.with_signer(
			Signer::try_from_secret(
				FUNDED_PRIVATE_KEYS[key as usize]
					.parse()
					.expect("invalid hardcoded builder private key"),
			)
			.expect("invalid hardcoded builder private key"),
		)
	}

	pub fn with_value(mut self, value: u128) -> Self {
		self.tx.value = U256::from(value);
		self
	}

	pub fn with_signer(mut self, signer: Signer) -> Self {
		self.signer = Some(signer);
		self
	}

	pub fn with_chain_id(mut self, chain_id: u64) -> Self {
		self.tx.chain_id = chain_id;
		self
	}

	pub fn with_nonce(mut self, nonce: u64) -> Self {
		self.tx.nonce = nonce;
		self
	}

	pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
		self.tx.gas_limit = gas_limit;
		self
	}

	pub fn with_max_fee_per_gas(mut self, max_fee_per_gas: u128) -> Self {
		self.tx.max_fee_per_gas = max_fee_per_gas;
		self
	}

	pub fn with_max_priority_fee_per_gas(
		mut self,
		max_priority_fee_per_gas: u128,
	) -> Self {
		self.tx.max_priority_fee_per_gas = max_priority_fee_per_gas;
		self
	}

	pub fn with_input(mut self, input: Bytes) -> Self {
		self.tx.input = input;
		self
	}

	pub fn with_revert(mut self) -> Self {
		self.tx.input = hex!("60006000fd").into();
		self
	}

	pub async fn build(mut self) -> Recovered<TxEnvelope> {
		if self.signer.is_none() {
			self = self.with_funded_account(0);
		}

		let signer = self.signer.unwrap();

		let nonce = match self.nonce {
			Some(nonce) => nonce,
			None => self
				.provider
				.get_transaction_count(signer.address)
				.pending()
				.await
				.expect("Failed to get transaction count"),
		};

		let base_fee = match self.base_fee {
			Some(base_fee) => base_fee,
			None => {
				let previous_base_fee = self
					.provider
					.get_block_by_number(BlockNumberOrTag::Latest)
					.await
					.expect("failed to get latest block")
					.expect("latest block should exist")
					.header
					.base_fee_per_gas
					.unwrap_or(MIN_PROTOCOL_BASE_FEE);

				max(previous_base_fee as u128, MIN_PROTOCOL_BASE_FEE as u128)
			}
		};

		self.tx.nonce = nonce;
		self.tx.max_fee_per_gas = base_fee + self.tx.max_priority_fee_per_gas;

		let signature_hash = self.tx.signature_hash();
		let signature = signer
			.sign_message(signature_hash)
			.expect("Failed to sign transaction hash");
		let signed_tx = TransactionSigned::new_unhashed(self.tx.into(), signature);

		Recovered::new_unchecked(signed_tx.into(), signer.address)
	}

	pub async fn send(self) -> eyre::Result<PendingTransactionBuilder<Ethereum>> {
		let provider = self.provider.clone();
		let transaction = self.build().await;
		let transaction_encoded = transaction.encoded_2718();

		Ok(
			provider
				.send_raw_transaction(transaction_encoded.as_slice())
				.await?,
		)
	}
}
