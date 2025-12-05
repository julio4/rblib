//! # Checkpoint Chain
//!
//! This module implements a chain of **checkpoints** used during payload
//! building. Each checkpoint represents the state *after* applying one mutation
//! (a transaction, a bundle, or a noop barrier), and cheap to clone, move, or
//! drop.
//!
//! The design supports two types of checkpoints:
//!
//! - **Light checkpoints** — store only their local state diff (`BundleState`).
//! - **Fat checkpoints** — additionally store an *accumulated state*, which is
//!   a fully squashed view of all diffs up to that point.
//!
//! Fat checkpoints act as **skip-list anchors**, speeding up state lookups and
//! reducing the cost of traversing the checkpoint chain.
//!
//! ## Checkpoint Chain Structure
//!
//! Each checkpoint internally stores:
//!
//! - `prev`: the previous checkpoint in the linear history,
//! - `mutation`: either a barrier or the execution result of a tx/bundle,
//! - `fat_ancestor`: an optional link to the *closest previous* fat checkpoint,
//! - `accumulated_state`: `None` for light checkpoints, `Some` for fat ones.
//!
//! The chain always forms a backward-linked list:
//!
//! ```text
//!   base_state
//!      |
//!      v
//!   [C1] <- [C2] <- [C3] <- [C4] <- [C5] <- [C6] <- ...
//! ```
//!
//! Fat checkpoints (*) introduce skip-edges
//!
//! ```text
//!   [C3]* <-----+
//!      ^        |
//!      |        |
//!   [C6]* ------+
//!      ^
//!      |
//!   [C8]
//! ```
//!
//! allowing fast traversal backward through accumulated state windows.
//!
//! ## How Accumulated State Is Built
//!
//! A checkpoint becomes *fat* when `.fat()` is invoked on it. The accumulation
//! logic depends on whether a fat ancestor exists.
//!
//! ### Case 1: No Fat Ancestor Exists
//!
//! This happens near the beginning of the block, before any fat checkpoint is
//! created. The accumulated state is built by squashing *all* diffs from the
//! start of the chain up to this checkpoint:
//!
//! ```text
//! accumulated = squash([base, state1, state2, state3])
//! ```
//!
//! This creates a baseline-accumulated snapshot.
//!
//! ### Case 2: A Fat Ancestor Exists
//!
//! Let the history be:
//!
//! ```text
//! base
//!  ├─ C1: state1
//!  ├─ C2: state2
//!  ├─ C3: state3  -> FAT, accumulated = squash([base,1,2,3])
//!  ├─ C4: state4
//!  ├─ C5: state5
//!  ├─ C6: state6  -> FAT
//!  ├─ C7: state7
//!  ├─ C8: state8
//! ```
//!
//! When C6 becomes fat, we **do not reuse accumulated** state1–state3.
//! Instead, we *start* from C4 and only apply from there as the fat ancestor
//! checkpoint already includes its own diff:
//!
//! ```text
//! base
//!  ├─ C1: state1
//!  ├─ C2: state2
//!  ├─ C3: state3  -> FAT, accumulated = squash([base,1,2,3])
//!  ├─ C4: state4
//!  ├─ C5: state5
//!  ├─ C6: state6  -> FAT, accumulated = squash([4,5,6])
//!  ├─ C7: state7
//!  ├─ C8: state8
//! ```
//!
//! ## State Lookup Logic
//!
//! Given a checkpoint `C9` (light), state access proceeds through these layers:
//!
//! 1. **Local mutation state** (`state9`)
//! 2. **Previous light checkpoints** (C8 and C7)
//! 3. **Hit a fat checkpoint (C6)**
//!    - check only its *accumulated* state (C4–C6)
//! 4. **Jump to `C6.fat_ancestor` -> C3**
//!    - Check only accumulated state (C1–C3)
//! 5. **Fall back to base state**
//!
//! This ensures:
//! - Lookups do not scan the entire chain,
//! - Fat checkpoints define "state windows" that are collapsed
//!
//! ## TLDR
//!
//! - Light checkpoints store only their local diff.
//! - Fat checkpoints store a squashed snapshot of all diffs since the previous
//!   fat checkpoint.
//! - `fat_ancestor` provides skip-list–style acceleration by linking fat
//!   checkpoints together.
//! - State lookup walks for light checkpoint:
//!   - local diffs
//!   - then local diffs of previous light checkpoints,
//!   - at the first fat checkpoint, the fat checkpoint accumulated diffs,
//!   - then jumps to earlier fat checkpoints accumulated diffs,
//!   - then base.
//!
//! This design supports efficient execution, simulation, and incremental block
//! building

use {
	super::exec::IntoExecutable,
	crate::{alloy, prelude::*, reth},
	alloy::{
		consensus::{crypto::RecoveryError, transaction::TxHashRef},
		primitives::{Address, B256, StorageValue},
	},
	core::fmt::{Debug, Display},
	reth::{
		errors::ProviderError,
		primitives::Recovered,
		revm::{
			DatabaseRef,
			db::BundleState,
			primitives::StorageKey,
			state::{AccountInfo, Bytecode},
		},
	},
	std::{iter::Successors, sync::Arc, time::Instant},
	thiserror::Error,
};

#[derive(Debug, Error)]
pub enum Error<P: Platform> {
	#[error("Failed to recover signature for transaction")]
	SignatureRecovery(#[from] RecoveryError),

	#[error("Evm execution error: {0}")]
	Evm(types::EvmError<P, ProviderError>),
}

/// Checkpoints represent an atomic incremental change in the payload building
/// process.
///
/// Notes:
///  - There is no public API to create a checkpoint directly. Checkpoints are
///    created from the [`BlockContext`] when it starts a new payload building
///    process or by mutations applied to an already existing checkpoint.
///
///  - Checkpoints contain all the information needed to assemble a full block
///    payload. However, they cannot be used directly to assemble a block. The
///    block assembly process is very node-specific and is part of the pipelines
///    api, which has more info and access to the underlying node facilities.
///
///  - Checkpoints are immutable, meaning that once a checkpoint is created, it
///    cannot be changed. Instead, new checkpoints can be created on top of the
///    existing ones, forming a chain of checkpoints.
///
///  - Checkpoints may represent forks in the payload building process. Two
///    checkpoints can share a common ancestor without having a linear history
///    between them. Each of the diverging checkpoints can be used to build
///    alternative versions of the payload.
///
///  - Checkpoints are inexpensive to clone, discard, and move around. However,
///    they are expensive to create, as they require executing transactions
///    through the EVM and storing the resulting state changes.
///
///  - Checkpoints are thread-safe, Send + Sync + 'static.
///
///  - Checkpoints are always in a state that can be used to build a valid block
///    payload. You can't create checkpoints with invalid transactions (such as
///    invalid nonces, invalid signatures, etc.) that would invalidate the block
///    payload validity according to consensus rules.
///
///  - Checkpoints are state providers, meaning that any checkpoint can be used
///    as a database reference in an input to an EVM instance when simulating
///    transactions. The state of the checkpoint is the cumulative state of all
///    state mutations applied since the beginning of the block payload,
///    including the base state of the parent block of the block for which the
///    payload is being built.
pub struct Checkpoint<P: Platform> {
	inner: Arc<CheckpointInner<P>>,
}

/// Public read API
impl<P: Platform> Checkpoint<P> {
	/// Returns the number of checkpoints preceding this checkpoint from the
	/// beginning of the block payload.
	///
	/// Depth zero is when [`BlockContext::start`] is called, and the first
	/// checkpoint is created with no previous checkpoints.
	pub fn depth(&self) -> usize {
		self.inner.depth
	}

	/// Returns the timestamp when this checkpoint was created.
	pub fn created_at(&self) -> Instant {
		self.inner.created_at
	}

	/// Returns the previous checkpoint before the current checkpoint.
	///
	/// Using the previous checkpoint is equivalent to discarding the
	/// state mutations made in the current checkpoint.
	///
	/// There may be multiple parallel forks of the payload under construction,
	/// rooted at the same checkpoint.
	pub fn prev(&self) -> Option<Checkpoint<P>> {
		self.inner.prev.as_ref().map(|prev| Checkpoint {
			inner: Arc::clone(prev),
		})
	}

	/// Returns the block context at the base of the checkpoint.
	pub fn block(&self) -> &BlockContext<P> {
		&self.inner.block
	}

	/// The transactions that created this checkpoint.
	/// The returned slice is a view into all applied transactions in this
	/// checkpoint:
	/// - Empty if this checkpoint is a barrier or other non-transaction
	///   checkpoint.
	/// - Single transaction if this checkpoint was created by applying a single
	///   transaction.
	/// - Multiple transactions if this checkpoint represents a bundle.
	pub fn transactions(&self) -> &[Recovered<types::Transaction<P>>] {
		match &self.inner.mutation {
			Mutation::Barrier => &[],
			Mutation::Executable(result) => result.transactions(),
		}
	}

	/// The execution result(s) of the transaction(s) that created this
	/// checkpoint.
	pub fn result(&self) -> Option<&ExecutionResult<P>> {
		match &self.inner.mutation {
			Mutation::Barrier => None,
			Mutation::Executable(result) => Some(result),
		}
	}

	/// The state changes that occurred as a result of executing the
	/// transaction(s) that created this checkpoint.
	///
	/// If this is a "fat" checkpoint with accumulated state, returns the
	/// accumulated state (which includes all states changes since last fat
	/// checkpoint including this local checkpoint's mutation state). Otherwise,
	/// returns just the local mutation's state.
	pub fn state(&self) -> Option<&BundleState> {
		// Return accumulated state if this is a fat checkpoint
		if let Some(ref accumulated) = self.inner.accumulated_state {
			return Some(accumulated);
		}

		// Otherwise return the local mutation's state
		match self.inner.mutation {
			Mutation::Barrier => None,
			Mutation::Executable(ref result) => Some(result.state()),
		}
	}

	/// Returns true if this checkpoint is a barrier checkpoint.
	pub fn is_barrier(&self) -> bool {
		matches!(self.inner.mutation, Mutation::Barrier)
	}

	/// Returns the context of this checkpoint.
	pub fn context(&self) -> &P::CheckpointContext {
		&self.inner.context
	}

	pub fn has_context(&self, context: &P::CheckpointContext) -> bool {
		self.context() == context
	}

	/// If this checkpoint is created from a single transaction, returns a
	/// reference to this transaction. Otherwise, returns `None`.
	pub fn as_transaction(&self) -> Option<&Recovered<types::Transaction<P>>> {
		if let Mutation::Executable(result) = &self.inner.mutation
			&& let Executable::Transaction(tx) = result.source()
		{
			return Some(tx);
		}
		None
	}

	/// If this checkpoint is created from a bundle, returns a reference to this
	/// bundle. Otherwise, returns `None`.
	pub fn as_bundle(&self) -> Option<&types::Bundle<P>> {
		if let Mutation::Executable(result) = &self.inner.mutation
			&& let Executable::Bundle(bundle) = result.source()
		{
			return Some(bundle);
		}
		None
	}
}

/// Public builder API
impl<P: Platform> Checkpoint<P> {
	/// Creates a new checkpoint on top of the current checkpoint by applying a
	/// transaction or a bundle of transactions. The execution will use the
	/// cumulative state of all checkpoints in the current checkpoint history as
	/// its state.
	pub fn apply<S>(
		&self,
		executable: impl IntoExecutable<P, S>,
	) -> Result<Self, ExecutionError<P>> {
		let mutation =
			Mutation::Executable(executable.try_into_executable()?.execute(
				self.block(),
				self,
				self.context(),
			)?);
		Ok(self.apply_with(mutation, self.context().clone()))
	}

	/// Creates a new checkpoint on top of the current checkpoint and tags it.
	/// The execution will use the cumulative state of all checkpoints in the
	/// current checkpoint history as its state.
	pub fn apply_with_context<S>(
		&self,
		executable: impl IntoExecutable<P, S>,
		context: P::CheckpointContext,
	) -> Result<Self, ExecutionError<P>> {
		let mutation =
			Mutation::Executable(executable.try_into_executable()?.execute(
				self.block(),
				self,
				&context,
			)?);
		Ok(self.apply_with(mutation, context))
	}

	/// Creates a new checkpoint on top of the current checkpoint that introduces
	/// a barrier. This new checkpoint will be now considered the new beginning of
	/// staging history.
	#[must_use]
	pub fn barrier(&self) -> Self {
		self.apply_with(Mutation::Barrier, self.context().clone())
	}

	/// Creates a new tagged barrier checkpoint on top of the current checkpoint.
	#[must_use]
	pub fn barrier_with_context(&self, context: P::CheckpointContext) -> Self {
		self.apply_with(Mutation::Barrier, context)
	}

	/// Given this checkpoint, this method builds a new payload on top of this
	/// block base state that is ready to be handed back to the CL client as a
	/// response to the `ForkchoiceUpdated` request.
	pub fn build_payload(
		&self,
	) -> Result<types::BuiltPayload<P>, PayloadBuilderError> {
		P::build_payload(self.clone(), self.block().base_state())
	}

	/// Creates a "fat" checkpoint with accumulated state.
	///
	/// This method traverses the checkpoint history to the latest fat ancestor
	/// and merges all state changes using `BundleState::extend` to create a
	/// single accumulated state. The resulting checkpoint can be used as a
	/// skip-list anchor point for efficient state lookups.
	///
	/// If this checkpoint already has accumulated state, it returns self
	/// unchanged.
	#[must_use]
	pub fn fat(mut self) -> Self {
		// If already a fat checkpoint, return self
		if self.inner.accumulated_state.is_some() {
			return self;
		}

		// Collect/clone state diffs from (latest fat ancestor, self]
		let mut states = self
			.iter_from_fat_ancestor()
			.filter_map(|cp| cp.result().map(|r| r.state().clone()));

		let Some(mut accumulated) = states.next() else {
			// no mutations (only barriers): don't make this fat.
			// TODO: maybe still return a fat checkpoint with an empty accumulated
			// state here?
			return self.clone();
		};

		// Extend with the rest of the diffs (each cloned once above).
		for state in states {
			accumulated.extend(state);
		}

		// Try to update in place if exclusive access to checkpoint inner
		if let Some(inner) = Arc::get_mut(&mut self.inner) {
			inner.accumulated_state = Some(accumulated);
			self
		} else {
			// Fallback: create a new CheckpointInner
			Self {
				inner: Arc::new(CheckpointInner {
					block: self.inner.block.clone(),
					prev: self.inner.prev.clone(),
					fat_ancestor: self.inner.fat_ancestor.clone(),
					depth: self.inner.depth,
					mutation: self.inner.mutation.clone(),
					accumulated_state: Some(accumulated),
					created_at: self.inner.created_at,
					context: self.inner.context.clone(),
				}),
			}
		}
	}
}

/// Internal API
impl<P: Platform> Checkpoint<P> {
	// Create a new checkpoint on top of the current one with the given mutation.
	// See public builder API.
	#[must_use]
	fn apply_with(
		&self,
		mutation: Mutation<P>,
		context: P::CheckpointContext,
	) -> Self {
		let fat_ancestor = if self.inner.accumulated_state.is_some() {
			Some(Arc::clone(&self.inner))
		} else {
			self.inner.fat_ancestor.clone()
		};

		Self {
			inner: Arc::new(CheckpointInner {
				block: self.inner.block.clone(),
				prev: Some(Arc::clone(&self.inner)),
				fat_ancestor,
				depth: self.inner.depth + 1,
				mutation,
				accumulated_state: None,
				created_at: Instant::now(),
				context,
			}),
		}
	}

	/// Start a new checkpoint for an empty payload rooted at the
	/// state of the parent block of the block for which the payload is
	/// being built.
	#[must_use]
	pub(super) fn new_at_block(block: BlockContext<P>) -> Self {
		Self {
			inner: Arc::new(CheckpointInner {
				block,
				prev: None,
				fat_ancestor: None,
				depth: 0,
				mutation: Mutation::Barrier,
				accumulated_state: None,
				created_at: Instant::now(),
				context: Default::default(),
			}),
		}
	}

	/// Start a new checkpoint but seeded with the provided `CheckpointContext`.
	#[must_use]
	pub(super) fn new_with_context(
		block: BlockContext<P>,
		context: P::CheckpointContext,
	) -> Self {
		Self {
			inner: Arc::new(CheckpointInner {
				block,
				prev: None,
				fat_ancestor: None,
				depth: 0,
				mutation: Mutation::Barrier,
				accumulated_state: None,
				created_at: Instant::now(),
				context,
			}),
		}
	}

	/// Lazy iterator over historic checkpoints.
	/// Note that it is in reverse history order, starting from the latest applied
	/// checkpoint up to the first one.
	#[allow(unused)]
	fn iter(&self) -> Successors<Self, fn(&Self) -> Option<Self>> {
		<&Self as IntoIterator>::into_iter(self)
	}

	/// Iterator from the latest fat ancestor (or base if none) to self in order
	/// of application NOT lazy, see `Self::iter` instead for lazy backward
	/// traversal.
	fn iter_from_fat_ancestor(&self) -> impl Iterator<Item = Checkpoint<P>> {
		let mut chain: Vec<_> = self
			.into_iter()
			.take_while(|cp| cp.inner.accumulated_state.is_none())
			.collect();
		chain.reverse(); // oldest -> newest
		chain.into_iter()
	}
}

impl<P: Platform> IntoIterator for &Checkpoint<P> {
	type IntoIter =
		Successors<Checkpoint<P>, fn(&Checkpoint<P>) -> Option<Checkpoint<P>>>;
	type Item = Checkpoint<P>;

	fn into_iter(self) -> Self::IntoIter {
		std::iter::successors(Some(self.clone()), |cp| {
			cp.inner.prev.as_ref().map(|prev| Checkpoint {
				inner: Arc::clone(prev),
			})
		})
	}
}

/// Describes the type of state mutation that was applied to the previous
/// checkpoint to create this checkpoint.
#[derive(Debug, Clone, PartialEq)]
enum Mutation<P: Platform> {
	/// A checkpoint that indicates that any prior checkpoints are immutable and
	/// should not be discarded or reordered. An example of this would be placing
	/// a barrier after applying sequencer transactions to ensure that they do
	/// not get reordered by pipelines. Another example would be placing a barrier
	/// after every committed flashblock to ensure that any steps in the pipeline
	/// do not modify the committed state of the payload in process.
	///
	/// If there are multiple barriers in the history, the last one is considered
	/// as the beginning of the staging history.
	///
	/// The very first checkpoint in the history is always a barrier, as it
	/// represents the baseline checkpoint that has no transactions in its
	/// history.
	Barrier,

	/// A checkpoint that was created by applying one executable item on top of
	/// the previous checkpoint. The executable item can be a single transaction
	/// or a bundle of transactions.
	Executable(ExecutionResult<P>),
}

struct CheckpointInner<P: Platform> {
	/// The block context for which this checkpoint was created.
	block: BlockContext<P>,

	/// The previous checkpoint in this chain of checkpoints, if any.
	prev: Option<Arc<Self>>,

	/// The latest "fat" checkpoint in this chain of checkpoints, if any.
	fat_ancestor: Option<Arc<Self>>,

	/// The number of checkpoints in the chain starting from the beginning of the
	/// block context.
	///
	/// Depth zero is when [`BlockContext::start`] is called, as the first
	/// checkpoint
	depth: usize,

	/// The mutation kind for the checkpoint.
	mutation: Mutation<P>,

	/// The accumulated state of the checkpoint, only present if the checkpoint
	/// is "fat"
	accumulated_state: Option<BundleState>,

	/// The timestamp when this checkpoint was created.
	created_at: Instant,

	/// User-defined context for this checkpoint.
	context: P::CheckpointContext,
}

/// Converts a checkpoint into a vector of transactions that were applied to
/// it.
impl<P: Platform> From<Checkpoint<P>> for Vec<types::Transaction<P>> {
	fn from(checkpoint: Checkpoint<P>) -> Self {
		checkpoint
			.history()
			.transactions()
			.map(Recovered::clone_inner)
			.collect()
	}
}

impl<P: Platform> CheckpointInner<P> {
	/// Traverse the checkpoint chain in the logical lookup order and apply `f`
	/// to each visible `BundleState`.
	///
	/// Semantics:
	/// - For light checkpoints: visit their local mutation `BundleState` (if
	///   any), then go to `prev`.
	/// - For fat checkpoints: visit their `accumulated_state`, then jump to
	///   `fat_ancestor`.
	/// - Stops as soon as `f` returns `Some(_)`.
	fn find_in_chain<T, F>(&self, mut f: F) -> Option<T>
	where
		F: FnMut(&BundleState) -> Option<T>,
	{
		let mut current: Option<&CheckpointInner<P>> = Some(self);

		while let Some(inner) = current {
			if let Some(ref accumulated) = inner.accumulated_state {
				// Fat checkpoint: check the accumulated state only, then jump to
				// fat_ancestor.
				if let Some(found) = f(accumulated) {
					return Some(found);
				}

				current = inner.fat_ancestor.as_deref();
			} else {
				// Light checkpoint: check the local mutation state only, then go to
				// prev.
				if let Mutation::Executable(ref result) = inner.mutation {
					let state = result.state();
					if let Some(found) = f(state) {
						return Some(found);
					}
				}

				current = inner.prev.as_deref();
			}
		}

		None
	}
}

/// `DatabaseRef` implementation for `CheckpointInner`.
/// This is the core implementation that efficiently traverses the checkpoint
/// chain using the skip-list structure (`fat_ancestor`) when available.
impl<P: Platform> DatabaseRef for CheckpointInner<P> {
	type Error = ProviderError;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		if let Some(account) = self.find_in_chain(|state| {
			state
				.account(&address)
				.and_then(|a| a.info.as_ref())
				.cloned()
		}) {
			return Ok(Some(account));
		}

		// Fallback to base state.
		if let Some(acc) = self.block.base_state().basic_account(&address)? {
			Ok(Some(acc.into()))
		} else {
			Ok(None)
		}
	}

	/// Gets account code by its hash.
	fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
		if let Some(code) = self.find_in_chain(|state| state.bytecode(&code_hash)) {
			return Ok(code);
		}

		// Fallback to base state bytecode.
		Ok(
			self
				.block
				.base_state()
				.bytecode_by_hash(&code_hash)?
				.unwrap_or_default()
				.0,
		)
	}

	/// Gets storage value of address at index.
	fn storage_ref(
		&self,
		address: Address,
		index: StorageKey,
	) -> Result<StorageValue, Self::Error> {
		if let Some(value) = self.find_in_chain(|state| {
			state
				.account(&address)
				.and_then(|a| a.storage.get(&index))
				.map(|slot| slot.present_value)
		}) {
			return Ok(value);
		}

		// Fallback to base state storage.
		Ok(
			self
				.block
				.base_state()
				.storage(address, index.into())?
				.unwrap_or_default(),
		)
	}

	fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
		Ok(
			self
				.block
				.base_state()
				.block_hash(number)?
				.unwrap_or_default(),
		)
	}
}

/// Any checkpoint can be used as a database reference for an EVM instance.
/// The state at a checkpoint is the cumulative aggregate of all state mutations
/// that occurred in the current checkpoint and all its ancestors on top of the
/// base state of the parent block of the block for which the payload is being
/// built.
/// See `<CheckpointInner as DatabaseRef>`
impl<P: Platform> DatabaseRef for Checkpoint<P> {
	/// The database error type.
	type Error = ProviderError;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		self.inner.basic_ref(address)
	}

	/// Gets account code by its hash.
	fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
		self.inner.code_by_hash_ref(code_hash)
	}

	/// Gets storage value of address at index.
	fn storage_ref(
		&self,
		address: Address,
		index: StorageKey,
	) -> Result<StorageValue, Self::Error> {
		self.inner.storage_ref(address, index)
	}

	/// Gets block hash by block number.
	fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
		self.inner.block_hash_ref(number)
	}
}

impl<P: Platform> Clone for Checkpoint<P> {
	fn clone(&self) -> Self {
		Self {
			inner: Arc::clone(&self.inner),
		}
	}
}

impl<P: Platform> PartialEq for Checkpoint<P> {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.inner, &other.inner)
	}
}

impl<P: Platform> Eq for Checkpoint<P> {}

impl<P: Platform> Debug for Checkpoint<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Checkpoint")
			.field("depth", &self.depth())
			.field("block", &format!("{} + 1", self.block().parent().hash()))
			.field("context", &self.context())
			.field(
				"txs",
				&self
					.transactions()
					.iter()
					.map(|tx| tx.tx_hash())
					.collect::<Vec<_>>(),
			)
			.field("result", &self.result())
			.finish()
	}
}

impl<P: Platform> Display for Checkpoint<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		let ctx_suffix = format!("{:?}", self.context());

		let Mutation::Executable(exec_result) = &self.inner.mutation else {
			if self.depth() == 0 {
				// this is the initial checkpoint
				return write!(f, "[{}] initial", self.depth());
			}

			// this is a barrier checkpoint, which has no transactions
			// applied to it.
			return match &self.inner.mutation {
				Mutation::Barrier => {
					write!(f, "[{}] barrier, context={}", self.depth(), ctx_suffix)
				}
				Mutation::Executable(_) => {
					unreachable!("Executable variant handled above")
				}
			};
		};

		match exec_result.source() {
			Executable::Transaction(tx) => write!(
				f,
				"[{}] tx {} ({}, {} gas)",
				self.depth(),
				tx.tx_hash(),
				match exec_result.results()[0] {
					types::TransactionExecutionResult::<P>::Success { .. } => "success",
					types::TransactionExecutionResult::<P>::Revert { .. } => "revert",
					types::TransactionExecutionResult::<P>::Halt { .. } => "halt",
				},
				self.gas_used(),
			),
			Executable::Bundle(bundle) => write!(
				f,
				"[{}] (bundle {} txs, {} gas) metadata={}",
				self.depth(),
				bundle.transactions().len(),
				self.gas_used(),
				ctx_suffix
			),
		}
	}
}

#[cfg(test)]
mod tests {
	use {
		crate::{
			payload::checkpoint::{Checkpoint, IntoExecutable, Mutation},
			prelude::*,
			test_utils::{
				BlockContextMocked,
				apply_multiple,
				test_bundle,
				test_tx,
				test_txs,
			},
		},
		std::time::Instant,
	};

	mod internal {
		use super::*;
		#[test]
		fn test_new_at_block() {
			// test the initial checkpoint with private `Checkpoint::new_at_block`
			let block = BlockContext::<Ethereum>::mocked();

			let before = Instant::now();
			let checkpoint = Checkpoint::new_at_block(block.clone());
			let after = Instant::now();

			assert_eq!(checkpoint.block(), &block);
			assert!(checkpoint.prev().is_none());
			assert_eq!(checkpoint.depth(), 0);
			assert!(checkpoint.is_barrier());
			assert!((before..=after).contains(&checkpoint.created_at()));
		}

		#[test]
		fn test_apply_with() {
			// test the checkpoint obtained by application with private
			// `Checkpoint::apply_with`
			let block = BlockContext::<Ethereum>::mocked();
			let root = block.start();

			let before = Instant::now();
			let checkpoint = root.barrier();
			let after = Instant::now();

			assert_eq!(checkpoint.block(), &block);
			assert_eq!(checkpoint.prev(), Some(root.clone()));
			assert_eq!(checkpoint.depth(), root.depth() + 1);
			assert!(checkpoint.is_barrier());
			assert!((before..=after).contains(&checkpoint.created_at()));
		}
	}

	#[test]
	fn test_apply_barrier() {
		let block = BlockContext::<Ethereum>::mocked();
		let root = block.start();
		let barrier = root.barrier();

		assert!(barrier.result().is_none());
		assert!(barrier.state().is_none());
		assert!(barrier.as_transaction().is_none());
		assert!(barrier.as_bundle().is_none());
		assert!(barrier.transactions().is_empty());
	}

	#[test]
	fn test_apply_tx() {
		// test the checkpoint obtained by application with `Checkpoint::apply`
		let block = BlockContext::<Ethereum>::mocked();
		let root = block.start();

		let tx = test_tx::<Ethereum>(0, 0);
		let checkpoint = root.apply(tx.clone()).unwrap();
		assert_eq!(checkpoint.as_transaction(), Some(&tx));
		assert_eq!(checkpoint.transactions(), std::slice::from_ref(&tx));
		assert!(checkpoint.as_bundle().is_none());
		assert!(!checkpoint.is_barrier());

		// expected mutation result
		let res = tx
			.try_into_executable()
			.unwrap()
			.execute(&block, &root, root.context())
			.unwrap();
		assert_eq!(checkpoint.result(), Some(&res));
		assert_eq!(checkpoint.state(), Some(res.state()));
		assert_eq!(checkpoint.transactions(), res.transactions());
		assert_eq!(checkpoint.inner.mutation, Mutation::Executable(res));

		let tx = test_tx::<Ethereum>(0, 1);
		let checkpoint = checkpoint.apply(tx.clone()).unwrap();
		assert_eq!(checkpoint.depth(), 2);

		// This tx is supposed to fail
		let fail_tx = tx;
		let checkpoint_res = checkpoint.apply(fail_tx);
		assert!(checkpoint_res.is_err());
	}

	#[test]
	fn test_apply_bundle() {
		let block = BlockContext::<Ethereum>::mocked();
		let root = block.start();

		let (bundle, txs) = test_bundle::<Ethereum>(0, 0);
		let checkpoint = root.apply(bundle.clone()).unwrap();
		assert_eq!(checkpoint.as_bundle(), Some(&bundle));
		assert_eq!(checkpoint.transactions(), txs.as_slice());
		assert!(checkpoint.as_transaction().is_none());
		assert!(!checkpoint.is_barrier());

		// expected mutation result
		let res = bundle
			.try_into_executable()
			.unwrap()
			.execute(&block, &root, root.context())
			.unwrap();
		assert_eq!(checkpoint.result(), Some(&res));
		assert_eq!(checkpoint.state(), Some(res.state()));
		assert_eq!(checkpoint.transactions(), res.transactions());
		assert_eq!(checkpoint.inner.mutation, Mutation::Executable(res));

		let (bundle, _) = test_bundle::<Ethereum>(0, 3);
		let checkpoint = checkpoint.apply(bundle.clone()).unwrap();
		assert_eq!(checkpoint.depth(), 2);

		// This bundle is supposed to fail
		let fail_bundle = bundle;
		let checkpoint_res = checkpoint.apply(fail_bundle);
		assert!(checkpoint_res.is_err());
	}

	#[test]
	fn test_iter() {
		let block = BlockContext::<Ethereum>::mocked();
		let root = block.start();

		let txs = test_txs::<Ethereum>(0, 0, 10);
		let checkpoints = apply_multiple(root, &txs);

		let latest = checkpoints.last().unwrap();
		let iter = latest.into_iter();

		assert!(
			checkpoints
				.into_iter()
				.rev()
				.zip(iter)
				.all(|(expected_cp, cp)| expected_cp == cp)
		);
	}

	#[test]
	fn test_checkpoint_to_txs() {
		let block = BlockContext::<Ethereum>::mocked();
		let root = block.start();
		let txs = test_txs::<Ethereum>(0, 0, 10);
		let checkpoint = apply_multiple(root, &txs).last().unwrap().to_owned();

		let extracted_txs = Vec::<types::Transaction<Ethereum>>::from(checkpoint);

		assert!(
			txs
				.into_iter()
				.map(|tx| tx.inner().clone())
				.zip(extracted_txs)
				.all(|(expected_tx, tx)| expected_tx == tx)
		);
	}

	#[test]
	fn test_build_payload() {
		let block = BlockContext::<Ethereum>::mocked();
		let provider = block.base_state();

		let root = block.start();
		let txs = test_txs::<Ethereum>(0, 0, 10);
		let checkpoint = apply_multiple(root, &txs).last().unwrap().to_owned();

		let built_payload = checkpoint.build_payload().unwrap();
		let payload = Ethereum::build_payload(checkpoint, provider).unwrap();

		assert_eq!(built_payload.id(), payload.id());
		assert_eq!(built_payload.block(), payload.block());
	}

	mod fat_checkpoints {
		use {
			super::*,
			crate::{
				alloy::primitives::{Address, B256},
				reth::revm::{DatabaseRef, primitives::U256},
			},
			std::sync::Arc,
		};

		/// return depths of checkpoints produced by `iter_from_fat_ancestor`.
		fn depths<P: Platform>(cp: &Checkpoint<P>) -> Vec<usize> {
			cp.iter_from_fat_ancestor().map(|c| c.depth()).collect()
		}

		#[test]
		fn fat_on_first_mutation_accumulates_from_base() {
			let block = BlockContext::<Ethereum>::mocked();
			let base = block.start(); // depth 0, barrier

			let txs = test_txs::<Ethereum>(0, 0, 3);
			let checkpoints = apply_multiple(base, &txs);
			let c1 = checkpoints[0].clone();
			let c2 = checkpoints[1].clone();
			let c3 = checkpoints[2].clone();

			// Sanity: all light, no accumulated state, no fat ancestors.
			assert!(c1.inner.accumulated_state.is_none());
			assert!(c2.inner.accumulated_state.is_none());
			assert!(c3.inner.accumulated_state.is_none());
			assert!(c1.inner.fat_ancestor.is_none());
			assert!(c2.inner.fat_ancestor.is_none());
			assert!(c3.inner.fat_ancestor.is_none());

			// Make C3 fat: with no existing fat ancestor, we should accumulate
			// the whole window from base to C3.
			let c3_fat = c3.clone().fat();
			assert!(c3_fat.inner.accumulated_state.is_some());
			// the first fat checkpoint has no fat_ancestor
			assert!(c3_fat.inner.fat_ancestor.is_none());

			// iter_from_fat_ancestor should cover all diffs from base to C3:
			// depths [0, 1, 2, 3] (Base, C1, C2, C3).
			let window_depths = depths(&c3);
			assert_eq!(window_depths, vec![0, 1, 2, 3]);

			// Public API should still work the same way.
			assert_eq!(c3_fat.depth(), c3.depth());
			assert_eq!(c3_fat.prev(), c3.prev());
			assert_eq!(c3_fat.as_transaction(), c3.as_transaction());
			assert!(c3_fat.state().is_some());
		}

		#[test]
		fn fat_with_existing_fat_ancestor_accumulates_only_last_window() {
			let block = BlockContext::<Ethereum>::mocked();
			let base = block.start();

			// Build 6 checkpoints.
			let txs = test_txs::<Ethereum>(0, 0, 6);
			let checkpoints = apply_multiple(base, &txs[0..3]);

			let c3 = checkpoints[2].clone(); // depth 3
			// First fat checkpoint at C3: accumulates [C1, C2, C3].
			let c3_fat = c3.clone().fat();

			let checkpoints = apply_multiple(c3_fat.clone(), &txs[3..6]);
			let c4 = checkpoints[3 - 3].clone(); // depth 4
			let c5 = checkpoints[4 - 3].clone(); // depth 5
			let c6 = checkpoints[5 - 3].clone(); // depth 6

			assert!(c3_fat.inner.accumulated_state.is_some());
			assert!(c3_fat.inner.fat_ancestor.is_none());

			// Sanity: the successors C4/C5/C6 should have C3 as fat_ancestor.
			assert!(Arc::ptr_eq(
				c4.inner
					.fat_ancestor
					.as_ref()
					.expect("expected fat ancestor on C4"),
				&c3_fat.inner
			));
			assert!(Arc::ptr_eq(
				c5.inner
					.fat_ancestor
					.as_ref()
					.expect("expected fat ancestor on C5"),
				&c3_fat.inner
			));
			assert!(Arc::ptr_eq(
				c6.inner
					.fat_ancestor
					.as_ref()
					.expect("expected fat ancestor on C6"),
				&c3_fat.inner
			));

			// Make C6 fat: now we should accumulate only the window [C4, C5, C6].
			let c6_fat = c6.clone().fat();
			assert!(c6_fat.inner.accumulated_state.is_some());

			// The fat ancestor of C6 must be C3.
			assert!(Arc::ptr_eq(
				c6_fat
					.inner
					.fat_ancestor
					.as_ref()
					.expect("fat ancestor on C6"),
				&c3_fat.inner
			));

			// iter_from_fat_ancestor on C6 should start after C3, i.e. depths [4, 5,
			// 6].
			let window_depths = depths(&c6);
			assert_eq!(window_depths, vec![4, 5, 6]);
		}

		#[test]
		fn iter_from_fat_ancestor_for_light_descendants_uses_latest_fat() {
			let block = BlockContext::<Ethereum>::mocked();
			let base = block.start();

			let txs = test_txs::<Ethereum>(0, 0, 10);
			let checkpoints = apply_multiple(base, &txs[0..3]);
			let c3 = checkpoints[2].clone();
			let c3_fat = c3.clone().fat();

			let checkpoints = apply_multiple(c3_fat.clone(), &txs[3..6]);
			let c6 = checkpoints[5 - 3].clone();
			let c6_fat = c6.clone().fat();

			let checkpoints = apply_multiple(c6_fat.clone(), &txs[6..10]);
			let c8 = checkpoints[7 - 3 - 3].clone();

			// After C6 becomes fat, later checkpoints should see C6 as their
			// latest fat ancestor, so the window for C8 is (C6, C8] -> depths [7, 8].
			let window_depths = depths(&c8);
			assert_eq!(window_depths, vec![7, 8]);

			// And for C6 itself, the window is from its fat ancestor C3:
			// depths [4, 5, 6].
			let window_depths_c6 = depths(&c6);
			assert_eq!(window_depths_c6, vec![4, 5, 6]);

			// For C3 (first fat), the window covers from base: [0, 1, 2, 3].
			let window_depths_c3 = depths(&c3);
			assert_eq!(window_depths_c3, vec![0, 1, 2, 3]);
		}

		#[test]
		fn database_ref_traversal_resolves_state_through_fat_windows() {
			let block = BlockContext::<Ethereum>::mocked();
			let base = block.start();

			// Build a moderately deep chain.
			let txs = test_txs::<Ethereum>(0, 0, 8);
			let checkpoints = apply_multiple(base, &txs);

			// Make two fat checkpoints as skip-list anchors.
			let _c3_fat = checkpoints[2].clone().fat();
			let _c6_fat = checkpoints[5].clone().fat();
			let latest = checkpoints[7].clone(); // C8

			// We just want to ensure we get the same via Checkpoint and via
			// CheckpointInner.
			let addr = Address::random();
			let key = U256::from(0);

			// basic_ref should never error
			let from_cp = latest.basic_ref(addr).unwrap();
			let from_inner = latest.inner.basic_ref(addr).unwrap();
			assert_eq!(from_cp.is_some(), from_inner.is_some());

			// storage_ref should never error as well.
			let storage_from_cp = latest.storage_ref(addr, key).unwrap();
			let storage_from_inner = latest.inner.storage_ref(addr, key).unwrap();
			assert_eq!(storage_from_cp, storage_from_inner);

			// code_by_hash_ref should be consistent.
			let any_hash = B256::random();
			let code_from_cp = latest.code_by_hash_ref(any_hash).unwrap();
			let code_from_inner = latest.inner.code_by_hash_ref(any_hash).unwrap();
			assert_eq!(code_from_cp, code_from_inner);
		}
	}
}
