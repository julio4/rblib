use {
	crate::*,
	alloy::{consensus::Transaction, primitives::TxHash},
	reth::primitives::Recovered,
	reth_ethereum::primitives::SignedTransaction,
	std::collections::{VecDeque, vec_deque::IntoIter},
	thiserror::Error,
};

#[derive(Debug, Clone, Error)]
pub enum Error {
	/// This error means that the one of the checkpoints is not a descendant of
	/// the other.
	#[error("There is no linear history between the checkpoints")]
	NonlinearHistory,
}

/// A span represents a sequence of checkpoints in the payload building process
/// with linear history. While checkpoints only allow to traverse the history
/// backwards, a span allows to traverse the history forwards as well.
///
/// Using a span you can also treat a number of checkpoints as a single
/// aggregate of state transitions.
pub struct Span<P: Platform> {
	checkpoints: VecDeque<Checkpoint<P>>,
}

/// Construction
impl<P: Platform> Span<P> {
	/// Attempts to create a span between two checkpoints.
	///
	/// The order of checkpoints does not matter as long as one of them is a
	/// descendant of the other.
	pub fn between(
		start: &Checkpoint<P>,
		end: &Checkpoint<P>,
	) -> Result<Self, Error> {
		let start = start.clone();
		let end = end.clone();

		if start == end {
			// it's a single-checkpoint span,
			// we can return it directly
			return Ok(Self {
				checkpoints: [start.clone()].into_iter().collect(),
			});
		}

		// find out which checkpoint is a descendant of the other
		let (ancestor, descendant) = if start.depth() < end.depth() {
			(start, end)
		} else {
			(end, start)
		};

		// Gather all checkpoints from the descendant to the ancestor
		// and ensure that the history is linear.

		let mut checkpoints = Vec::with_capacity(
			descendant
				.depth()
				.saturating_sub(ancestor.depth())
				.saturating_add(1),
		);

		let mut current = descendant;
		checkpoints.push(current.clone());

		while let Some(prev) = current.prev() {
			if prev.depth() < ancestor.depth() {
				return Err(Error::NonlinearHistory);
			}

			checkpoints.push(prev.clone());

			if prev == ancestor {
				// we've reached the ancestor checkpoint and we have linear history. The
				// collected checkpoints are in reverse order, so we need to reverse
				// them before returning.
				return Ok(Self {
					checkpoints: checkpoints.into_iter().rev().collect(),
				});
			}

			current = prev;
		}

		Err(Error::NonlinearHistory)
	}
}

/// Iteration
impl<P: Platform> Span<P> {
	/// The number of checkpoints in the span.
	pub fn len(&self) -> usize {
		self.checkpoints.len()
	}

	/// Returns `true` if the span is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.checkpoints.is_empty()
	}

	/// Returns the first checkpoint in the span
	pub fn first(&self) -> Option<&Checkpoint<P>> {
		self.checkpoints.front()
	}

	/// Returns the last checkpoint in the span.
	pub fn last(&self) -> Option<&Checkpoint<P>> {
		self.checkpoints.back()
	}

	/// Removes the first checkpoint from the span and returns it.
	/// Returns `None` if the span is empty.
	pub fn pop_first(&mut self) -> Option<Checkpoint<P>> {
		self.checkpoints.pop_front()
	}

	/// Returns a span with the last checkpoint removed.
	pub fn pop_last(&mut self) -> Option<Checkpoint<P>> {
		self.checkpoints.pop_back()
	}

	/// Returns a checkpoint at the given index relative to the start of the span.
	pub fn at(&self, index: usize) -> Option<&Checkpoint<P>> {
		self.checkpoints.get(index)
	}

	/// Checks if this span contains a checkpoint with a transaction with a given
	/// hash.
	pub fn contains(&self, txhash: impl Into<TxHash>) -> bool {
		let hash = txhash.into();
		self.checkpoints.iter().any(|checkpoint| {
			checkpoint
				.transaction()
				.map(|tx| hash == *tx.tx_hash())
				.unwrap_or(false)
		})
	}

	/// Returns an iterator over the checkpoints in the span.
	/// The iteration order is from the ancestor checkpoint to the descendant
	/// checkpoint.
	pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Checkpoint<P>> {
		self.checkpoints.iter()
	}

	/// Iterates of all transactions in the span in chronological order as they
	/// appear in the payload under construction.
	///
	/// This iterator returns a reference to each transaction.
	pub fn transactions(
		&self,
	) -> impl Iterator<Item = &Recovered<types::Transaction<P>>> {
		self
			.iter()
			.filter_map(|checkpoint| checkpoint.transaction())
	}

	/// Iterates over all blob transactions in the span.
	pub fn blobs(
		&self,
	) -> impl Iterator<Item = &Recovered<types::Transaction<P>>> {
		self
			.transactions()
			.filter(|tx| tx.blob_gas_used().is_some())
	}
}

/// Stats
impl<P: Platform> Span<P> {
	/// Returns the total gas used by all checkpoints in the span.
	pub fn gas_used(&self) -> u64 {
		self
			.checkpoints
			.iter()
			.map(|checkpoint| checkpoint.gas_used())
			.sum()
	}

	/// Returns the total blob gas used by all blob transactions in the span.
	pub fn blob_gas_used(&self) -> u64 {
		self
			.checkpoints
			.iter()
			.filter_map(|checkpoint| {
				checkpoint.transaction().and_then(|tx| tx.blob_gas_used())
			})
			.sum()
	}
}

impl<P: Platform> IntoIterator for Span<P> {
	type IntoIter = IntoIter<Checkpoint<P>>;
	type Item = Checkpoint<P>;

	fn into_iter(self) -> Self::IntoIter {
		self.checkpoints.into_iter()
	}
}
