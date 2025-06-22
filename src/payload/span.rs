use {
	crate::{payload::checkpoint::Mutation, *},
	alloc::vec::Vec,
	alloy::primitives::address,
	reth::primitives::Recovered,
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
	checkpoints: Vec<Checkpoint<P>>,
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
				checkpoints: alloc::vec![start.clone()],
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

	/// Never returns true, as a span always contains at least one checkpoint.
	pub fn is_empty(&self) -> bool {
		assert!(!self.checkpoints.is_empty());
		false
	}

	/// Returns an iterator over the checkpoints in the span.
	/// The iteration order is from the ancestor checkpoint to the descendant
	/// checkpoint.
	pub fn iter(&self) -> impl Iterator<Item = &Checkpoint<P>> {
		Iter {
			checkpoints: &self.checkpoints,
			index: 0,
		}
	}

	/// Iterates of all transactions in the span in chronological order as they
	/// appear in the payload under construction.
	///
	/// This iterator returns a reference to each transaction.
	pub fn transactions(&self) -> impl Iterator<Item = &types::Transaction<P>> {
		self
			.iter()
			.filter_map(|checkpoint| match checkpoint.mutation() {
				Mutation::Transaction { content, .. } => Some(content),
				_ => None,
			})
	}

	/// Iterates over all transactions in the span and recovers their senders.
	/// This iterator returns a copy of each transaction.
	pub fn transactions_recovered(
		&self,
	) -> impl Iterator<Item = Recovered<&types::Transaction<P>>> {
		self.transactions().filter_map(|tx| {
			Some(Recovered::new_unchecked(
				tx,
				address!("0x1234567890abcdef1234567890abcdef12345678"),
			))
		})
	}
}

/// An iterator over checkpoints in a span.
/// The iteration order is from the ancestor checkpoint to the descendant
/// checkpoint.
pub struct Iter<'a, P: Platform> {
	checkpoints: &'a [Checkpoint<P>],
	index: usize,
}

impl<'a, P: Platform> Iterator for Iter<'a, P> {
	type Item = &'a Checkpoint<P>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.index < self.checkpoints.len() {
			let checkpoint = &self.checkpoints[self.index];
			self.index += 1;
			Some(checkpoint)
		} else {
			None
		}
	}
}

impl<'a, P: Platform> DoubleEndedIterator for Iter<'a, P> {
	fn next_back(&mut self) -> Option<Self::Item> {
		if self.index > 0 {
			self.index -= 1;
			Some(&self.checkpoints[self.index])
		} else {
			None
		}
	}
}

impl<'a, P: Platform> ExactSizeIterator for Iter<'a, P> {
	fn len(&self) -> usize {
		self.checkpoints.len()
	}
}

impl<P: Platform> IntoIterator for Span<P> {
	type IntoIter = alloc::vec::IntoIter<Checkpoint<P>>;
	type Item = Checkpoint<P>;

	fn into_iter(self) -> Self::IntoIter {
		self.checkpoints.into_iter()
	}
}
