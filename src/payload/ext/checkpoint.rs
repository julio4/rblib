use crate::{
	Checkpoint,
	Platform,
	Span,
	SpanError,
	SpanExt,
	alloy::consensus::Transaction,
	reth::revm::context::result::ExecutionResult,
};

/// Quality of Life extensions for the `Checkpoint` type.
pub trait CheckpointExt<P: Platform>: super::sealed::Sealed {
	/// Returns `true` if this checkpoint is the baseline checkpoint in the
	/// history, and has no transactions in its history.
	fn is_empty(&self) -> bool;

	/// Returns the first checkpoint in the chain of checkpoints since the
	/// beginning of the block payload we're building.
	#[must_use]
	fn root(&self) -> Self;

	/// Gas used by this checkpoint.
	fn gas_used(&self) -> u64;

	/// Returns the effective tip for this transaction.
	fn effective_tip_per_gas(&self) -> u128;

	/// If this checkpoint was created by applying a blob transaction,
	/// returns the blob gas used by the blob transaction, `None` otherwise.
	fn blob_gas_used(&self) -> Option<u64>;

	/// Returns `true` if the transaction that created this checkpoint was
	/// successful, `false` otherwise.
	fn is_success(&self) -> bool;

	/// Returns `true` if this checkpoint was created by applying a
	/// transaction, `false` otherwise.
	fn is_transaction(&self) -> bool;

	/// Returns `true` if this checkpoint was created by applying EIP-4844 blob
	/// transaction, `false` otherwise.
	fn is_blob(&self) -> bool;

	/// Returns a span that includes all checkpoints from the beginning of the
	/// block payload we're building to the current checkpoint.
	fn history(&self) -> Span<P>;

	/// Returns a span that includes all mutable history of this checkpoint,
	/// which is all checkpoints from the last barrier checkpoint to this
	/// checkpoint, or the entire history if there is no barrier checkpoint.
	fn history_mut(&self) -> Span<P> {
		let history = self.history();
		let immutable_prefix = history
			.iter()
			.rposition(Checkpoint::is_barrier)
			.unwrap_or(0);
		history.skip(immutable_prefix)
	}

	/// Returns a span that includes all checkpoints in the immutable history,
	/// that is the history from the beginning of the block until the last
	/// barrier included. If there are no barriers, the entire history is
	/// returned.
	fn history_const(&self) -> Span<P> {
		let history = self.history();
		let immutable_prefix = history
			.iter()
			.rposition(Checkpoint::is_barrier)
			.unwrap_or(0);
		history.take(immutable_prefix + 1)
	}

	/// Creates a new span that includes this checkpoints and all other
	/// checkpoints that are between this checkpoint and the given checkpoint.
	///
	/// The two checkpoints must be part of the same linear history, meaning that
	/// one of them must be a descendant of the other.
	///
	/// The other checkpoint can be either a previous or a future checkpoint.
	fn to(&self, other: &Self) -> Result<Span<P>, SpanError>;
}

impl<P: Platform> CheckpointExt<P> for Checkpoint<P> {
	/// Returns `true` if this checkpoint is the baseline checkpoint in the
	/// history, and has no transactions in its history.
	fn is_empty(&self) -> bool {
		self.depth() == 0
	}

	/// Returns the first checkpoint in the chain of checkpoints since the
	/// beginning of the block payload we're building.
	fn root(&self) -> Checkpoint<P> {
		let mut current = self.clone();
		while let Some(prev) = current.prev() {
			current = prev;
		}
		current
	}

	/// Gas used by this checkpoint.
	fn gas_used(&self) -> u64 {
		self.result().map_or(0, ExecutionResult::gas_used)
	}

	/// Returns the effective tip for this transaction.
	fn effective_tip_per_gas(&self) -> u128 {
		self
			.transaction()
			.and_then(|tx| tx.effective_tip_per_gas(self.block().base_fee()))
			.unwrap_or(0)
	}

	/// If this checkpoint was created by applying an EIP-4844 blob transaction,
	/// returns the blob gas used, `None` otherwise.
	fn blob_gas_used(&self) -> Option<u64> {
		self.transaction().and_then(|tx| tx.blob_gas_used())
	}

	/// Returns `true` if the transaction that created this checkpoint was
	/// successful, `false` otherwise.
	fn is_success(&self) -> bool {
		self.result().is_none_or(ExecutionResult::is_success)
	}

	/// Returns `true` if this checkpoint was created by applying a
	/// transaction, `false` otherwise.
	fn is_transaction(&self) -> bool {
		self.transaction().is_some()
	}

	/// Returns `true` if this checkpoint was created by applying an EIP-4844 blob
	/// transaction, `false` otherwise.
	fn is_blob(&self) -> bool {
		self.blob_gas_used().is_some()
	}

	/// Returns a span that includes all checkpoints from the beginning of the
	/// block payload we're building to the current checkpoint.
	///
	/// This span is guaranteed to always have at least one checkpoint,
	/// which is the baseline checkpoint that is the root of the
	/// checkpoint history.
	fn history(&self) -> Span<P> {
		Span::between(self, &self.root())
			.expect("history is always linear between self and root")
	}

	/// Creates a new span that includes this checkpoints and all other
	/// checkpoints that are between this checkpoint and the given checkpoint.
	///
	/// The two checkpoints must be part of the same linear history, meaning that
	/// one of them must be a descendant of the other.
	///
	/// The other checkpoint can be either a previous or a future checkpoint.
	fn to(&self, other: &Checkpoint<P>) -> Result<Span<P>, SpanError> {
		Span::between(self, other)
	}
}
