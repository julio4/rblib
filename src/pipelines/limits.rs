use core::fmt::Debug;

/// This trait defines the limits that can be applied to the block building
/// process.
pub trait Limits: Debug + Send + Sync + 'static {
	/// The maximum cumulative gas that can be used in the block.
	/// This includes all transactions, epilogues, prologues, and other
	/// gas-consuming operations.
	fn gas_limit(&self) -> u64;

	/// The maximum number of transactions that can be included in the block.
	/// This is not a standard Etheremum limit, but implementations of this trait
	/// may choose to enforce it.
	fn max_transactions(&self) -> Option<u64> {
		None
	}

	/// The maximum size of a blob in a single transaction.
	fn max_blob_size(&self) -> u64 {
		// per EIP-4844, the maximum blob size is 128kB
		128 * 1024
	}

	/// The maximum number of blob transactions that can be included in the block.
	fn max_blob_transactions(&self) -> u64 {
		// per EIP-4844, the maximum number of blob transactions is 6
		6
	}

	/// The maximum cumulative size of all blobs in the block.
	fn max_total_blobs_size(&self) -> Option<u64> {
		Some(self.max_blob_size() * self.max_blob_transactions())
	}
}

#[derive(Debug, Clone)]
pub struct StaticLimits {
	/// The maximum cumulative gas that can be used in the block.
	pub gas_limit: u64,
}

impl Limits for StaticLimits {
	fn gas_limit(&self) -> u64 {
		self.gas_limit
	}
}
