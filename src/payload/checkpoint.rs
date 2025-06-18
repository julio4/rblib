use {
	super::{block::BlockContext, span, Platform, Span},
	crate::types,
	alloc::sync::Arc,
	alloy::primitives::{Address, StorageValue, B256, U256},
	reth::{
		api::PayloadBuilderError,
		revm::{
			context::result::ResultAndState,
			db::DBErrorMarker,
			state::{AccountInfo, Bytecode},
			DatabaseRef,
		},
	},
	thiserror::Error,
};

#[derive(Debug, Clone, Error)]
pub enum Error {}
impl DBErrorMarker for Error {}

pub struct Checkpoint<P: Platform> {
	inner: Arc<CheckpointInner<P>>,
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

/// Public read API
impl<P: Platform> Checkpoint<P> {
	/// Returns the number of checkpoints preceeding this checkpoint since the
	/// beginning of the block payload we're building.
	///
	/// Depth zero is when [`BlockContext::start`] is called, and the first
	/// checkpoint is created and has no previous checkpoints.
	pub fn depth(&self) -> usize {
		self.inner.depth
	}

	/// The returns the payload version before the current checkpoint.
	/// Working with the previous checkpoint is equivalent to discarding the
	/// state mutations made in the current checkpoint.
	///
	/// There may be multiple parallel forks of the payload under construction,
	/// rooted at the same checkpoint.
	pub fn prev(&self) -> Option<Checkpoint<P>> {
		self.inner.prev.as_ref().map(|prev| Checkpoint {
			inner: Arc::clone(prev),
		})
	}

	/// Creates a new span that includes this checkpoints and all other
	/// checkpoints that are between this checkpoint and the given checkpoint.
	///
	/// The two checkpoints must be part of the same linear history, meaning that
	/// one of them must be a descendant of the other.
	///
	/// The other checkpoint can be either a previous or a future checkpoint.
	pub fn to(&self, other: &Checkpoint<P>) -> Result<Span<P>, span::Error> {
		Span::between(self, other)
	}

	/// Returns the first checkpoint in the chain of checkpoints since the
	/// beginning of the block payload we're building.
	pub fn root(&self) -> Checkpoint<P> {
		let mut current = self.clone();
		while let Some(prev) = current.prev() {
			current = prev;
		}
		current
	}
}

/// Public builder API
impl<P: Platform> Checkpoint<P> {
	pub fn apply(
		&self,
		transaction: types::Transaction<P>,
	) -> Result<Self, Error> {
		Ok(Self {
			inner: Arc::new(CheckpointInner {
				block: self.inner.block.clone(),
				prev: Some(Arc::clone(&self.inner)),
				depth: self.inner.depth + 1,
				mutation: todo!(),
			}),
		})
	}

	/// Inserts a barrier on top of the current checkpoint.
	///
	/// A barrier is a special type of checkpoint that prevents any mutations to
	/// the checkpoints payload prior to it.
	pub fn barrier(&self) -> Self {
		Self::new_barrier(&self)
	}

	/// Produces a built payload that can be submitted as the result of a block
	/// building process.
	///
	/// This is an expensive operation and ideally it should be called only once
	/// at the end of the payload building process, when all the
	/// modifications to the payload are done.
	pub fn build(self) -> Result<types::BuiltPayload<P>, PayloadBuilderError> {
		let payload_id = self.inner.block.payload_id();

		// select all checkpoints since the beginning of the block
		// payload we're building, including the current one.
		let span = Span::between(&self, &self.root())
			.expect("history is always linear between self and root");

		todo!("Checkpoint::build not implemented yet")
	}
}

/// Internal API
impl<P: Platform> Checkpoint<P> {
	/// Start a new checkpoint for an empty payload rooted at the
	/// state of the parent block of the block for which the payload is
	/// being built.
	pub(super) fn new_at_block(block: BlockContext<P>) -> Self {
		Self {
			inner: Arc::new(CheckpointInner {
				block,
				prev: None,
				depth: 0,
				mutation: Mutation::Barrier,
			}),
		}
	}

	/// Creates a new checkpoint that is a barrier on top of the previous
	/// checkpoint.
	pub(super) fn new_barrier(prev: &Self) -> Self {
		Self {
			inner: Arc::new(CheckpointInner {
				block: prev.inner.block.clone(),
				prev: Some(Arc::clone(&prev.inner)),
				depth: prev.inner.depth + 1,
				mutation: Mutation::Barrier,
			}),
		}
	}
}

/// Describes the type of state mutation that created a given checkpoint.
enum Mutation<P: Platform> {
	/// A barrier was inserted on top of the previous checkpoint.
	///
	/// Barriers are used to prevent transactions from being reordered or the
	/// checkpoints payload from being mutated in any way prior to it.
	///
	/// This is also the type of mutation that is used to create the first
	/// checkpoint in the chain, which is created using [`BlockContext::start`].
	///
	/// A barrier at checkpoint zero is a noop, it does not prevent any
	/// transactions from being reordered.
	Barrier,

	/// A new transaction was applied on top of the previous checkpoint.
	Transaction {
		/// The transaction that was applied on top of the previous checkpoint.
		content: types::Transaction<P>,

		/// The result of executing the transaction, including the state changes
		/// and the result of the execution.
		result: ResultAndState,
	},
}

struct CheckpointInner<P: Platform> {
	/// The block context for which this checkpoint was created.
	block: BlockContext<P>,

	/// The previous checkpoint in this chain of checkpoints, if any.
	prev: Option<Arc<Self>>,

	/// The number of checkpoints in the chain starting from the begining of the
	/// block context.
	///
	/// Depth zero is when [`BlockContext::start`] is called, and the first
	depth: usize,

	/// The type of mutation that created this checkpoint.
	mutation: Mutation<P>,
}

impl<P: Platform> DatabaseRef for Checkpoint<P> {
	/// The database error type.
	type Error = Error;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		todo!()
	}

	/// Gets account code by its hash.
	fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
		todo!()
	}

	/// Gets storage value of address at index.
	fn storage_ref(
		&self,
		address: Address,
		index: U256,
	) -> Result<StorageValue, Self::Error> {
		todo!()
	}

	/// Gets block hash by block number.
	fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
		todo!()
	}
}
