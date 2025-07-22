use {
	crate::*,
	alloy::{
		consensus::BlockHeader,
		eips::{Decodable2718, Encodable2718},
		network::eip2718::Eip2718Error,
		primitives::TxHash,
	},
	core::{
		convert::Infallible,
		ops::{Deref, DerefMut},
	},
	reth::{
		ethereum::primitives::{
			Recovered,
			SignerRecoverable,
			crypto::RecoveryError,
		},
		rpc::types::mev::EthSendBundle,
	},
	serde::{Deserialize, Deserializer, Serialize, Serializer},
};

#[derive(Debug, Clone, Default)]
pub struct OpBundle {
	inner: EthSendBundle,
	recovered: Vec<Recovered<types::Transaction<Optimism>>>,
}

impl OpBundle {
	pub const fn inner(&self) -> &EthSendBundle {
		&self.inner
	}

	pub fn into_inner(self) -> EthSendBundle {
		self.inner
	}

	#[must_use]
	pub fn with_transaction(
		mut self,
		tx: Recovered<types::Transaction<Optimism>>,
	) -> Self {
		if !self.recovered.iter().any(|t| t.tx_hash() == tx.tx_hash()) {
			let txbytes = tx.encoded_2718();
			self.inner.txs.push(txbytes.into());
			self.recovered.push(tx);
		}
		self
	}

	#[must_use]
	pub fn with_optional_transaction(
		self,
		tx: Recovered<types::Transaction<Optimism>>,
	) -> Self {
		let hash = tx.tx_hash();
		self.with_transaction(tx).mark_as_optional(hash)
	}

	#[must_use]
	pub fn mark_as_revertable(mut self, tx: TxHash) -> Self {
		if !self.inner.reverting_tx_hashes.contains(&tx) {
			self.inner.reverting_tx_hashes.push(tx);
		}
		self
	}

	#[must_use]
	pub fn mark_as_optional(mut self, tx: TxHash) -> Self {
		if !self.inner.dropping_tx_hashes.contains(&tx) {
			self.inner.dropping_tx_hashes.push(tx);
		}
		self
	}

	#[must_use]
	pub fn with_max_timestamp(mut self, max_ts: u64) -> Self {
		self.inner.max_timestamp = Some(max_ts);
		self
	}

	#[must_use]
	pub fn with_min_timestamp(mut self, min_ts: u64) -> Self {
		self.inner.min_timestamp = Some(min_ts);
		self
	}

	#[must_use]
	pub fn with_block_number(mut self, block_number: u64) -> Self {
		self.inner.block_number = block_number;
		self
	}
}

impl Bundle<Optimism> for OpBundle {
	type PostExecutionError = Infallible;

	fn transactions(&self) -> &[Recovered<types::Transaction<Optimism>>] {
		&self.recovered
	}

	fn without_transaction(self, tx: TxHash) -> Self {
		let mut bundle = self;
		if let Some(position) =
			bundle.recovered.iter().position(|t| t.tx_hash() == tx)
		{
			bundle.inner.txs.remove(position);
			bundle.recovered.remove(position);
		}

		bundle
			.reverting_tx_hashes
			.retain(|&reverting_tx| reverting_tx != tx);

		bundle
			.dropping_tx_hashes
			.retain(|&dropping_tx| dropping_tx != tx);

		bundle
	}

	fn is_eligible(&self, block: &BlockContext<Optimism>) -> Eligibility {
		if self.transactions().is_empty() {
			// empty bundles are never eligible
			return Eligibility::PermanentlyIneligible;
		}

		if self
			.max_timestamp
			.is_some_and(|max_ts| max_ts > block.timestamp())
		{
			// this bundle will never be eligible for inclusion anymore
			return Eligibility::PermanentlyIneligible;
		}

		if self
			.min_timestamp
			.is_some_and(|min_ts| min_ts < block.timestamp())
		{
			// this bundle is not eligible yet
			return Eligibility::TemporarilyIneligible;
		}

		if self.block_number != 0 && self.block_number > block.parent().number() {
			// this bundle is not eligible yet
			return Eligibility::TemporarilyIneligible;
		}

		if self.block_number != 0 && self.block_number < block.parent().number() {
			// this bundle will never be eligible for inclusion anymore
			return Eligibility::PermanentlyIneligible;
		}

		Eligibility::Eligible
	}

	fn is_allowed_to_fail(&self, tx: TxHash) -> bool {
		self.reverting_tx_hashes.contains(&tx)
	}

	fn is_optional(&self, tx: TxHash) -> bool {
		self.dropping_tx_hashes.contains(&tx)
	}
}

#[derive(Debug, thiserror::Error)]
pub enum BundleConversionError {
	#[error("EIP-2718 decoding error: {0}")]
	Decoding(#[from] Eip2718Error),

	#[error("Invalid transaction signature: {0}")]
	Signature(#[from] RecoveryError),
}

impl Serialize for OpBundle {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		self.inner.serialize(serializer)
	}
}

impl<'de> Deserialize<'de> for OpBundle {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let inner = EthSendBundle::deserialize(deserializer)?;
		Self::try_from(inner).map_err(serde::de::Error::custom)
	}
}

impl Deref for OpBundle {
	type Target = EthSendBundle;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl DerefMut for OpBundle {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.inner
	}
}

impl TryFrom<EthSendBundle> for OpBundle {
	type Error = BundleConversionError;

	fn try_from(inner: EthSendBundle) -> Result<Self, Self::Error> {
		let txs = inner
			.txs
			.iter()
			.map(|tx| types::Transaction::<Optimism>::decode_2718(&mut &tx[..]))
			.collect::<Result<Vec<_>, _>>()?;

		let recovered = txs
			.into_iter()
			.map(|tx| tx.try_into_recovered())
			.collect::<Result<Vec<_>, _>>()?;

		Ok(Self { inner, recovered })
	}
}
