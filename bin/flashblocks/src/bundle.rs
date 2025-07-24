use {
	crate::platform::FlashBlocks,
	alloy::{
		eips::Decodable2718,
		primitives::{B256, Bytes},
	},
	core::convert::Infallible,
	rblib::{
		alloy::{consensus::BlockHeader, primitives::TxHash},
		reth::{core::primitives::SignerRecoverable, primitives::Recovered},
		*,
	},
	serde::{Deserialize, Deserializer, Serialize, Serializer},
};

/// Represents a bundle of transactions.
///
/// This type is received from the `eth_sendBundle` RPC method.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlashBlocksBundle {
	/// The list of transactions in the bundle.
	///
	/// Notes:
	///  - The transactions are EIP-2718 encoded when serialized.
	///  - Bundles must contain at least one transaction.
	#[serde(with = "tx_encoding")]
	pub txs: Vec<Recovered<types::Transaction<FlashBlocks>>>,

	/// The list of transaction hashes in this bundle that are allowed to revert.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub reverting_tx_hashes: Vec<B256>,

	/// The list of transaction hashes in this bundle that are allowed to be
	/// removed from the bundle.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub dropping_tx_hashes: Vec<TxHash>,

	#[serde(
		default,
		with = "alloy_serde::quantity::opt",
		skip_serializing_if = "Option::is_none"
	)]
	pub min_block_number: Option<u64>,

	#[serde(
		default,
		with = "alloy_serde::quantity::opt",
		skip_serializing_if = "Option::is_none"
	)]
	pub max_block_number: Option<u64>,

	// Not recommended because this is subject to the builder node clock
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub min_timestamp: Option<u64>,

	// Not recommended because this is subject to the builder node clock
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_timestamp: Option<u64>,
}

/// Implements rblib Bundle semantics for the FlashBlocksBundle type.
impl Bundle<FlashBlocks> for FlashBlocksBundle {
	type PostExecutionError = Infallible;

	fn transactions(&self) -> &[Recovered<types::Transaction<FlashBlocks>>] {
		&self.txs
	}

	fn without_transaction(self, tx: TxHash) -> Self {
		let mut bundle = self;

		bundle.txs.retain(|t| t.tx_hash() != tx);

		bundle
			.reverting_tx_hashes
			.retain(|&reverting_tx| reverting_tx != tx);

		bundle
			.dropping_tx_hashes
			.retain(|&dropping_tx| dropping_tx != tx);

		bundle
	}

	fn is_eligible(&self, block: &BlockContext<FlashBlocks>) -> Eligibility {
		if self.txs.is_empty() {
			// empty bundles are never eligible
			return Eligibility::PermanentlyIneligible;
		}

		if self
			.max_block_number
			.is_some_and(|max_bn| max_bn < block.parent().number())
		{
			// this bundle will never be eligible for inclusion anymore
			return Eligibility::PermanentlyIneligible;
		}

		if self
			.min_block_number
			.is_some_and(|min_bn| min_bn > block.parent().number())
		{
			// this bundle is not eligible yet
			return Eligibility::TemporarilyIneligible;
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

		Eligibility::Eligible
	}

	fn is_allowed_to_fail(&self, tx: TxHash) -> bool {
		self.reverting_tx_hashes.contains(&tx)
	}

	fn is_optional(&self, tx: TxHash) -> bool {
		self.dropping_tx_hashes.contains(&tx)
	}
}

/// Implements the encoding and decoding of transactions to and from EIP-2718
/// hex bytes.
mod tx_encoding {
	use {super::*, rblib::alloy::eips::Encodable2718};

	type TxType = Recovered<types::Transaction<FlashBlocks>>;

	pub fn serialize<S>(
		txs: &Vec<TxType>,
		serializer: S,
	) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		if txs.is_empty() {
			return Err(serde::ser::Error::custom("empty transactions list"));
		}

		let encoded: Vec<Bytes> =
			txs.iter().map(|tx| tx.encoded_2718().into()).collect();
		encoded.serialize(serializer)
	}

	pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<TxType>, D::Error>
	where
		D: Deserializer<'de>,
	{
		let encoded = Vec::<Bytes>::deserialize(deserializer)?;

		if encoded.is_empty() {
			return Err(serde::de::Error::custom("empty transactions list"));
		}

		let txs = encoded
			.into_iter()
			.map(|tx| types::Transaction::<FlashBlocks>::decode_2718(&mut &tx[..]))
			.collect::<Result<Vec<_>, _>>()
			.map_err(serde::de::Error::custom)?;

		let recovered = txs
			.into_iter()
			.map(SignerRecoverable::try_into_recovered)
			.collect::<Result<Vec<_>, _>>()
			.map_err(serde::de::Error::custom)?;

		Ok(recovered)
	}
}
