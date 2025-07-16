use {
	crate::*,
	alloy::{
		consensus::BlockHeader,
		primitives::{Address, B256},
	},
	core::fmt::Display,
	reth::revm::{
		DatabaseRef,
		primitives::{StorageKey, StorageValue},
		state::{AccountInfo, Bytecode},
	},
	reth_errors::ProviderError,
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
///
/// A span can be used as a database reference for an evm instance. When a span
/// is used as a database, only new state that was created or modified by
/// checkpoints in the span are visible, any state from checkpoints outside of
/// the span or the base state of the block is not available.
#[derive(Clone)]
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

/// Public APIs
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

	/// Returns an iterator over the checkpoints in the span.
	/// The iteration order is from the ancestor checkpoint to the descendant
	/// checkpoint.
	pub fn iter(
		&self,
	) -> impl DoubleEndedIterator<Item = &Checkpoint<P>> + ExactSizeIterator + Clone
	{
		self.checkpoints.iter()
	}
}

/// Internal APIs
impl<P: Platform> Span<P> {
	/// Creates a span from an iterator of checkpoints.
	///
	/// This is an internal method and does not check if checkpoints
	/// form a linear history. It's the caller's responsibility to ensure
	/// that the checkpoints are in the correct order and form a linear history.
	pub(crate) unsafe fn from_iter_unchecked(
		checkpoints: impl Iterator<Item = Checkpoint<P>>,
	) -> Self {
		Self {
			checkpoints: checkpoints.collect(),
		}
	}
}

impl<P: Platform> IntoIterator for Span<P> {
	type IntoIter = IntoIter<Checkpoint<P>>;
	type Item = Checkpoint<P>;

	fn into_iter(self) -> Self::IntoIter {
		self.checkpoints.into_iter()
	}
}

impl<P: Platform> Display for Span<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		if self.is_empty() {
			return write!(f, "[empty]");
		}

		write!(
			f,
			"[checkpoints {}-{} / {} txs] ({} gas, {} blob gas)",
			&self.first().unwrap().depth(),
			&self.last().unwrap().depth(),
			&self.transactions().count(),
			&self.gas_used(),
			&self.blob_gas_used()
		)
	}
}

/// The state provided by a span is limited to state mutations that
/// were generated by the checkpoints in the span.
///
/// It's not a good idea to use a span as a database for EVM execution,
/// because it won't have access to any of the contract code or state
/// that was not created or modified by the checkpoints in the span.
impl<P: Platform> DatabaseRef for Span<P> {
	type Error = StateError;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		for checkpoint in self.checkpoints.iter().rev() {
			if let Some(account) =
				checkpoint.state().and_then(|state| state.get(&address))
			{
				return Ok(Some(account.info.clone()));
			}
		}

		Ok(None)
	}

	/// Gets account code by its hash.
	fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
		// we want to probe the history of checkpoints in reverse order,
		// starting from the most recent one, to find the first checkpoint
		// that has created the code with the given hash.
		// TODO: This is highly inefficient, optimize this asap.

		for checkpoint in self.checkpoints.iter().rev() {
			for account in checkpoint.state().iter().flat_map(|state| state.values())
			{
				if account.info.code_hash == code_hash {
					return Ok(
						account
							.info
							.code
							.as_ref()
							.expect("Code should be present")
							.clone(),
					);
				}
			}
		}

		Ok(Bytecode::default())
	}

	/// Gets storage value of address at index.
	fn storage_ref(
		&self,
		address: Address,
		index: StorageKey,
	) -> Result<StorageValue, Self::Error> {
		for checkpoint in self.checkpoints.iter().rev() {
			if let Some(account) =
				checkpoint.state().and_then(|state| state.get(&address))
			{
				return Ok(
					account
						.storage
						.get(&index)
						.map(|slot| slot.present_value)
						.unwrap_or_default(),
				);
			}
		}
		Ok(StorageValue::default())
	}

	/// Gets block hash by block number. In a span we only have access to the
	/// block hash of the parent block on top of which we are building the
	/// payload.
	fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
		let first_checkpoint = self
			.first()
			.ok_or(ProviderError::StateAtBlockPruned(number))?;

		if first_checkpoint.block().parent().number() != number {
			// if the request is not for the parent block,
			// we don't have the block hash in the span.
			return Err(ProviderError::StateAtBlockPruned(number).into());
		}

		Ok(first_checkpoint.block().parent().hash())
	}
}
