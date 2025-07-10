use {
	crate::{payload::span, *},
	alloy::primitives::{Address, B256, KECCAK256_EMPTY, StorageValue, U256},
	alloy_evm::{Evm, block::BlockExecutorFactory, evm::EvmFactory},
	core::fmt::{Debug, Display},
	reth::{
		api::ConfigureEvm,
		core::primitives::SignedTransaction,
		primitives::Recovered,
		providers::ProviderError,
		revm::{
			DatabaseRef,
			db::{DBErrorMarker, WrapDatabaseRef},
			state::{AccountInfo, Bytecode, EvmState},
		},
	},
	reth_evm::EvmError,
	std::sync::Arc,
	thiserror::Error,
};

pub type EvmFactoryError<P: Platform> =
	<
		<
			<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
		>::EvmFactory as EvmFactory
	>::Error<StateError>;

pub type InvalidTransactionError<P: Platform> =
	<EvmFactoryError<P> as EvmError>::InvalidTransaction;

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

	#[error("Invalid transaction: {0}")]
	InvalidTransaction(InvalidTransactionError<P>),
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

	/// The transaction that created this checkpoint.
	pub fn transaction(&self) -> Option<&Recovered<types::Transaction<P>>> {
		match &self.inner.mutation {
			Mutation::Initial => None,
			Mutation::Transaction { transaction, .. } => Some(transaction),
		}
	}

	/// The execution result of the transaction that created this checkpoint.
	pub fn result(&self) -> Option<&ExecutionResult<P>> {
		match &self.inner.mutation {
			Mutation::Initial => None,
			Mutation::Transaction { result, .. } => Some(&result.result),
		}
	}

	/// The state changes that occured as a result of executing the
	/// transaction that created this checkpoint.
	pub fn state(&self) -> Option<&EvmState> {
		match &self.inner.mutation {
			Mutation::Initial => None,
			Mutation::Transaction { result, .. } => Some(&result.state),
		}
	}

	/// Gas used by this checkpoint.
	pub fn gas_used(&self) -> u64 {
		self.result().map(|result| result.gas_used()).unwrap_or(0)
	}

	/// Returns `true` if the transaction that created this checkpoint was
	/// successful, `false` otherwise.
	pub fn is_success(&self) -> bool {
		self
			.result()
			.map(|result| result.is_success())
			.unwrap_or(true)
	}
}

/// Public builder API
impl<P: Platform> Checkpoint<P> {
	pub fn apply<S>(
		&self,
		transaction: impl IntoRecoveredTx<P, S>,
	) -> Result<Self, Error<P>> {
		let transaction = transaction.try_into_recovered()?;

		// Create a new EVM instance with its state rooted at the current checkpoint
		// state and the environment configured for the block under construction.
		let mut evm = self
			.block()
			.evm_config()
			.evm_with_env(WrapDatabaseRef(self), self.block().evm_env().clone());

		let result = evm.transact(&transaction).map_err(|err| {
			match err.try_into_invalid_tx_err() {
				Ok(invalid_tx) => Error::InvalidTransaction(invalid_tx),
				Err(e) => Error::EvmFactory(e),
			}
		})?;

		Ok(Self {
			inner: Arc::new(CheckpointInner {
				block: self.inner.block.clone(),
				prev: Some(Arc::clone(&self.inner)),
				depth: self.inner.depth + 1,
				mutation: Mutation::Transaction {
					transaction,
					result,
				},
			}),
		})
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
				mutation: Mutation::Initial,
			}),
		}
	}
}

enum Mutation<P: Platform> {
	/// An initial checkpoint that was created for an empty payload.
	/// This mutation type is only valid for checkpoints at depth zero.
	Initial,

	/// A regular checkpoint that was created by applying a transaction.
	Transaction {
		/// The transaction that was applied on top of the previous checkpoint.
		transaction: Recovered<types::Transaction<P>>,
		/// The result of executing the transaction, including the state changes
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

	/// The mutation
	mutation: Mutation<P>,
}

/// Converts a checkpoint into a vector of transactions that were applied to
/// it.
impl<P: Platform> From<Checkpoint<P>> for Vec<types::Transaction<P>> {
	fn from(checkpoint: Checkpoint<P>) -> Self {
		checkpoint
			.history()
			.transactions()
			.map(|tx| tx.clone_inner())
			.collect()
	}
}

/// Convinience trait that allows various variants of transactions to be used
/// as a parameter to the `Checkpoint::apply` method.
pub trait IntoRecoveredTx<P: Platform, Marker = ()>: Send + Sync {
	fn try_into_recovered(
		self,
	) -> Result<Recovered<types::Transaction<P>>, Error<P>>;
}

impl<P: Platform> IntoRecoveredTx<P, ()> for types::Transaction<P> {
	fn try_into_recovered(
		self,
	) -> Result<Recovered<types::Transaction<P>>, Error<P>> {
		SignedTransaction::try_into_recovered(self)
			.map_err(|_| Error::InvalidSignature)
	}
}

impl<P: Platform> IntoRecoveredTx<P, u8> for Recovered<types::Transaction<P>> {
	fn try_into_recovered(
		self,
	) -> Result<Recovered<types::Transaction<P>>, Error<P>> {
		Ok(self)
	}
}

impl<P: Platform> DatabaseRef for Checkpoint<P> {
	/// The database error type.
	type Error = StateError;

	/// Gets basic account information.
	fn basic_ref(
		&self,
		address: Address,
	) -> Result<Option<AccountInfo>, Self::Error> {
		// we want to probe the history of checkpoints in reverse order,
		// starting from the most recent one, to find the first checkpoint
		// that has touched the given address.
		for checkpoint in self.history().into_iter().rev() {
			if let Some(account) =
				checkpoint.state().and_then(|state| state.get(&address))
			{
				return Ok(Some(account.info.clone()));
			}
		}

		// none of the checkpoints priori to this have touched this address,
		// now we need to check if the account exists in the base state of the
		// block context.
		if let Some(acc) = self.block().base_state().basic_account(&address)? {
			return Ok(Some(AccountInfo {
				balance: acc.balance,
				nonce: acc.nonce,
				code_hash: acc.bytecode_hash.unwrap_or(KECCAK256_EMPTY),
				code: self
					.block()
					.base_state()
					.account_code(&address)?
					.map(|code| code.0),
			}));
		}

		// account does not exist
		Ok(None)
	}

	/// Gets account code by its hash.
	fn code_by_hash_ref(
		&self,
		_code_hash: B256,
	) -> Result<Bytecode, Self::Error> {
		todo!()
	}

	/// Gets storage value of address at index.
	fn storage_ref(
		&self,
		_address: Address,
		_index: U256,
	) -> Result<StorageValue, Self::Error> {
		todo!()
	}

	/// Gets block hash by block number.
	fn block_hash_ref(&self, _number: u64) -> Result<B256, Self::Error> {
		todo!()
	}
}

impl<P: Platform> Debug for Checkpoint<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Checkpoint")
			.field("depth", &self.depth() as &dyn Debug)
			.field("block", &self.block() as &dyn Debug)
			.field("tx", &self.transaction() as &dyn Debug)
			.field("result", &self.result() as &dyn Debug)
			.field("state", &self.state() as &dyn Debug)
			.finish()
	}
}

impl<P: Platform> Display for Checkpoint<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"[{}] {} ({}, {} gas)",
			self.depth(),
			&self
				.transaction()
				.map(|tx| tx.tx_hash().to_string())
				.unwrap_or_default(),
			match self.result() {
				Some(ExecutionResult::<P>::Success { .. }) => "success",
				Some(ExecutionResult::<P>::Revert { .. }) => "revert",
				Some(ExecutionResult::<P>::Halt { .. }) => "halt",
				None => "initial",
			},
			self.gas_used(),
		)
	}
}

type ResultAndState<P: Platform> = reth::revm::context::result::ResultAndState<
	<
		<
			<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
		>::EvmFactory as EvmFactory
	>::HaltReason
>;

type ExecutionResult<P: Platform> = reth::revm::context::result::ExecutionResult
	<
		<
			<
				<P::EvmConfig as ConfigureEvm>::BlockExecutorFactory as BlockExecutorFactory
			>::EvmFactory as EvmFactory
		>::HaltReason
	>;
