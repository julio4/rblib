use {
	crate::{alloy::eips::eip7840::BlobParams, prelude::*},
	std::time::Instant,
};

pub trait LimitsFactory<P: Platform>: Send + Sync + 'static {
	/// Configure the limits for the block payload under construction.
	///
	/// As an input this method takes the block context that we're producing a
	/// payload for and optionally any limits imposed by enclosing pipelines.
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits;
}

#[derive(Debug, Clone)]
pub struct Limits {
	/// The maximum cumulative gas that can be used in the block.
	/// This includes all transactions, epilogues, prologues, and other
	/// gas-consuming operations.
	pub gas_limit: u64,

	/// Limits for blob transactions in the block.
	pub blob_params: Option<BlobParams>,

	/// The maximum number of transactions that can be included in the block.
	///
	/// This is not a standard known ethereum limit, however it can be imposed by
	/// custom limits factories.
	pub max_transactions: Option<usize>,

	/// The time by which the payload must be built.
	///
	/// In most cases, the pipeline executor will stop iterating over loops if
	/// the deadline is reached, however for long running steps, its recommended
	/// to have deadline-aware logic inside the step itself.
	pub deadline: Option<Instant>,
}

impl Limits {
	pub fn with_gas_limit(gas_limit: u64) -> Self {
		Self {
			gas_limit,
			blob_params: None,
			deadline: None,
			max_transactions: None,
		}
	}

	#[must_use]
	pub fn with_blob_params(mut self, blob_params: BlobParams) -> Self {
		self.blob_params = Some(blob_params);
		self
	}

	#[must_use]
	pub fn with_max_transactions(mut self, max_transactions: usize) -> Self {
		self.max_transactions = Some(max_transactions);
		self
	}

	#[must_use]
	pub fn with_deadline(mut self, deadline: Instant) -> Self {
		self.deadline = Some(deadline);
		self
	}

	#[must_use]
	pub fn clamp(self, other: &Self) -> Self {
		Self {
			gas_limit: self.gas_limit.min(other.gas_limit),
			blob_params: match (self.blob_params, other.blob_params) {
				(Some(self_params), Some(other_params)) => Some(BlobParams {
					target_blob_count: self_params
						.target_blob_count
						.min(other_params.target_blob_count),
					max_blob_count: self_params
						.max_blob_count
						.min(other_params.max_blob_count),
					update_fraction: self_params
						.update_fraction
						.min(other_params.update_fraction),
					min_blob_fee: self_params.min_blob_fee.min(other_params.min_blob_fee),
					max_blobs_per_tx: self_params
						.max_blobs_per_tx
						.min(other_params.max_blobs_per_tx),
				}),
				_ => self.blob_params.or(other.blob_params),
			},
			max_transactions: self.max_transactions.min(other.max_transactions),
			deadline: self.deadline.min(other.deadline),
		}
	}
}
