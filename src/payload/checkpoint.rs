use {
	crate::*,
	alloy::primitives::{Address, B256, KECCAK256_EMPTY, StorageValue, U256},
	alloy_evm::{Evm, block::BlockExecutorFactory, evm::EvmFactory},
	core::fmt::{Debug, Display},
	reth::{
		api::ConfigureEvm,
		core::primitives::SignedTransaction,
		primitives::Recovered,
		revm::{
			DatabaseRef,
			db::WrapDatabaseRef,
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

/// Checkpoints represent an atomic incremental change in the payload building
/// process.
///
/// Notes:
///  - There is no public API to create a checkpoint directly. Checkpoints are
///    created by the [`BlockContext`] when it starts a new payload building
///    process or by mutatations applied to an already existing checkpoint.
///
///  - Checkpoints contain all the information needed to assemble a full block
///    payload, they however cannot be used directly to assemble a block. The
///    block assembly process is very node-specific and is part of the pipelines
///    api, which has more info and access to the underlying node facilities.
///
///  - Checkpoints are immutable, meaning that once a checkpoint is created, it
///    cannot be changed. Instead, new checkpoints can be created on top of the
///    existing ones, forming a chain of checkpoints.
///
///  - Checkpoints may represent forks in the payload building process. Two
///    checkpoints can share a common ancestor, without having linear history
///    between them. Each of the diverging checkpoints can be used to build
///    alternative versions of the payload.
///
///  - Checkpoints are cheap to clone, discard and move around. They are
///    expensive to create, as they require executing a transaction by the EVM
///    and storing the resulting state changes.
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

	/// Returns the block context that is the root of theis checkpoint.
	pub fn block(&self) -> &BlockContext<P> {
		&self.inner.block
	}

	/// The transaction that created this checkpoint.
	pub fn transaction(&self) -> Option<&Recovered<types::Transaction<P>>> {
		match &self.inner.mutation {
			Mutation::Baseline => None,
			Mutation::Transaction { transaction, .. } => Some(transaction),
		}
	}

	/// The execution result of the transaction that created this checkpoint.
	pub fn result(&self) -> Option<&ExecutionResult<P>> {
		match &self.inner.mutation {
			Mutation::Baseline => None,
			Mutation::Transaction { result, .. } => Some(&result.result),
		}
	}

	/// The state changes that occured as a result of executing the
	/// transaction that created this checkpoint.
	pub fn state(&self) -> Option<&EvmState> {
		match &self.inner.mutation {
			Mutation::Baseline => None,
			Mutation::Transaction { result, .. } => Some(&result.state),
		}
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
		Self {
			inner: Arc::new(CheckpointInner {
				block,
				prev: None,
				depth: 0,
				mutation: Mutation::Baseline,
			}),
		}
	}
}

/// Describes the type of state mutation that was applied to the
/// previous checkpoint to create this checkpoint.
enum Mutation<P: Platform> {
	/// An initial checkpoint that was created for an empty payload on top of the
	/// parent block state. This mutation type is only valid for checkpoints at
	/// depth zero.
	Baseline,

	/// A regular checkpoint that was created by applying a transaction.
	Transaction {
		/// The transaction that was applied on top of the previous checkpoint.
		transaction: Recovered<types::Transaction<P>>,
		/// The result of executing the transaction, including the state changes
		result: ResultAndState<P>,
	},
}

/// This is
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
			.map(Recovered::clone_inner)
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

/// Any checkpoint can be used as a database reference for an EVM instance.
/// The state at a checkpoint is the cumulative aggregate of all state mutations
/// that occured in the current checkpoint and all its ancestors on top of the
/// base state of the parent block of the block for which the payload is being
/// built.
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
	fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
		// we want to probe the history of checkpoints in reverse order,
		// starting from the most recent one, to find the first checkpoint
		// that has created the code with the given hash.
		// TODO: This is highly inefficient, optimize this asap.

		for checkpoint in self.history().into_iter().rev() {
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

		Ok(
			self
				.block()
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
		index: U256,
	) -> Result<StorageValue, Self::Error> {
		// traverse checkpoints history looking for the first checkpoint that
		// has touched the given address.
		for checkpoint in self.history().into_iter().rev() {
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

		// none of the checkpoints prior to this have touched this address,
		// now we need to check if the account exists in the base state of the
		// block context.
		Ok(
			self
				.block()
				.base_state()
				.storage(address, index.into())?
				.unwrap_or_default(),
		)
	}

	/// Gets block hash by block number.
	fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
		Ok(
			self
				.block()
				.base_state()
				.block_hash(number)?
				.unwrap_or_default(),
		)
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
		let (Some(tx), Some(result)) = (self.transaction(), self.result()) else {
			return write!(f, "[{}] (initial)", self.depth());
		};

		write!(
			f,
			"[{}] {} ({}, {} gas)",
			self.depth(),
			tx.tx_hash(),
			match result {
				ExecutionResult::<P>::Success { .. } => "success",
				ExecutionResult::<P>::Revert { .. } => "revert",
				ExecutionResult::<P>::Halt { .. } => "halt",
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
