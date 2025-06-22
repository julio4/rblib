use {
	crate::{payload::span, *},
	alloc::sync::Arc,
	alloy::primitives::{Address, StorageValue, B256, KECCAK256_EMPTY, U256},
	alloy_evm::{block::BlockExecutorFactory, evm::EvmFactory, Evm},
	reth::{
		api::{ConfigureEvm, PayloadBuilderError},
		core::primitives::SignedTransaction,
		providers::ProviderError,
		revm::{
			db::{DBErrorMarker, WrapDatabaseRef},
			state::{AccountInfo, Bytecode},
			DatabaseRef,
		},
	},
	thiserror::Error,
	tracing::info,
};

#[allow(type_alias_bounds)]
pub type EvmFactoryError<P: Platform> =
	<
		<
			<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
		>::EvmFactory as EvmFactory
	>::Error<StateError>;

#[derive(Debug, Error)]
pub enum Error<P: Platform> {
	#[error("state error: {0}")]
	State(#[from] StateError),

	#[error("transaction has invalid signature")]
	InvalidSignature,

	#[error("Evm execution error: {0}")]
	Evm(types::EvmError<P>),

	#[error("Evm factory error: {0}")]
	EvmFactory(EvmFactoryError<P>),
}

#[derive(Debug, Clone, Error)]
pub enum StateError {
	#[error("Provider error: {0}")]
	Provider(#[from] ProviderError),
}

impl DBErrorMarker for StateError {}

/// Notes:
///  - There is no public API to create a checkpoint directly. Checkpoints are
///    created by the [`BlockContext`] when it starts a new payload building
///    process or by mutatations applied to an already existing checkpoint.
///
///  - Checkpoints contain all the information needed to assemble a full block
///    payload, they however cannot be used durectly to build a block. The block
///    building process is very node-specific and is part of the pipelines api,
///    which has more info and access to the underlying node facilities.
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

	/// Returns a span that includes all checkpoints from the beginning of the
	/// block payload we're building to the current checkpoint.
	pub fn history(&self) -> Span<P> {
		Span::between(self, &self.root())
			.expect("history is always linear between self and root")
	}

	/// Returns the block context that is the root of theis checkpoint.
	pub fn block(&self) -> &BlockContext<P> {
		&self.inner.block
	}
}

/// Public builder API
impl<P: Platform> Checkpoint<P> {
	pub fn apply(
		&self,
		transaction: types::Transaction<P>,
	) -> Result<Self, Error<P>> {
		let Ok(recovered) = transaction.clone().try_into_recovered() else {
			return Err(Error::InvalidSignature);
		};

		// Create a new EVM instance with its state rooted at the current checkpoint
		// state and the environment configured for the block under construction.
		let mut evm = self.block_context().evm_config().evm_with_env(
			WrapDatabaseRef(self),
			self.block_context().evm_env().clone(),
		);

		let result = evm
			.transact(recovered)
			.map_err(|e| Error::<P>::EvmFactory(e))?;

		info!("Transaction result: {transaction:#?} ----> {result:#?}");

		Ok(Self {
			inner: Arc::new(CheckpointInner {
				block: self.inner.block.clone(),
				prev: Some(Arc::clone(&self.inner)),
				depth: self.inner.depth + 1,
				mutation: Mutation::Transaction {
					content: transaction,
					result,
				},
			}),
		})
	}

	/// Inserts a barrier on top of the current checkpoint.
	///
	/// A barrier is a special type of checkpoint that prevents any mutations to
	/// the checkpoints payload prior to it.
	pub fn barrier(&self) -> Self {
		Self::new_barrier(self)
	}
}

/// Internal API
impl<P: Platform> Checkpoint<P> {
	/// Start a new checkpoint for an empty payload rooted at the
	/// state of the parent block of the block for which the payload is
	/// being built.
	pub(super) fn new_at_block(block: BlockContext<P>) -> Self {
		// at the beginning of the payload building process, we need to apply any
		// pre-execution changes.

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

	pub(super) fn block_context(&self) -> &BlockContext<P> {
		&self.inner.block
	}

	pub(super) fn mutation(&self) -> &Mutation<P> {
		&self.inner.mutation
	}
}

/// Describes the type of state mutation that created a given checkpoint.
pub(super) enum Mutation<P: Platform> {
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
		result: ResultAndState<P>,
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
	type Error = StateError;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		for checkpoint in self.history() {
			match checkpoint.mutation() {
				Mutation::Barrier => continue,
				Mutation::Transaction { result, .. } => {
					if let Some(account) = result.state.get(&address) {
						return Ok(Some(account.info.clone()));
					}
				}
			};
		}

		// none of the checkpoints priori to this have touched this address,
		// now we need to check if the account exists in the base state of the
		// block context.
		if let Some(acc) =
			self.block_context().base_state().basic_account(&address)?
		{
			return Ok(Some(AccountInfo {
				balance: acc.balance,
				nonce: acc.nonce,
				code_hash: acc.bytecode_hash.unwrap_or(KECCAK256_EMPTY),
				code: self
					.block_context()
					.base_state()
					.account_code(&address)?
					.map(|code| code.0),
			}));
		}

		// account does not exist
		Ok(None)
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

#[allow(type_alias_bounds)]
type TypedError<P: Platform> =
	Error<
		<
			<
				<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
			>::EvmFactory as EvmFactory
		>::Error<StateError>
	>;

#[allow(type_alias_bounds)]
type ResultAndState<P: Platform> = reth::revm::context::result::ResultAndState<
	<
		<
			<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
		>::EvmFactory as EvmFactory
	>::HaltReason
>;
