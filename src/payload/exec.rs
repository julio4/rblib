use {
	crate::*,
	alloy::consensus::crypto::RecoveryError,
	reth::{
		errors::ProviderError,
		ethereum::primitives::SignedTransaction,
		evm::{
			ConfigureEvm,
			Evm,
			revm::{DatabaseRef, state::EvmState},
		},
		primitives::Recovered,
		revm::db::WrapDatabaseRef,
	},
	std::fmt::Debug,
};

/// Describes an atomic unit of execution that can be used to create a state
/// transition checkpoint.
#[derive(Debug, Clone)]
pub enum Executable<P: Platform> {
	// Individual transaction
	Transaction(Recovered<types::Transaction<P>>),
	// A bundle of transactions with its metadata
	Bundle(types::Bundle<P>),
}

impl<P: Platform> Executable<P> {
	pub fn execute<'a, DB>(
		self,
		block: &'a BlockContext<P>,
		db: &'a DB,
	) -> Result<ExecutionResult<P>, types::EvmError<P, ProviderError>>
	where
		DB: DatabaseRef<Error = ProviderError> + Debug + Send + Sync + 'static,
	{
		match self {
			Self::Transaction(tx) => Self::execute_tx(tx, block, db),
			Self::Bundle(bundle) => Self::execute_bundle(bundle, block, db),
		}
	}

	/// Executes a single transactions and creates a new checkpoint on top of the
	/// current checkpoint with the result of the transaction execution.
	///
	/// Transactions that cause evm errors cannot create checkpoints. This does
	/// not mean that checkpoints cannot have reverted or halted transactions.
	/// Only transactions that violate consensus rules are not allowed to create
	/// checkpoints, this includes things like invalid nonces, or others from
	/// [`reth_evm::revm::context::result::InvalidTransaction`].
	fn execute_tx<'a, DB>(
		tx: Recovered<types::Transaction<P>>,
		block: &'a BlockContext<P>,
		db: &'a DB,
	) -> Result<ExecutionResult<P>, types::EvmError<P, ProviderError>>
	where
		DB: DatabaseRef<Error = ProviderError> + Debug + Send + Sync + 'static,
	{
		// Create a new EVM instance with its state rooted at the current checkpoint
		// state and the environment configured for the block under construction.
		let result = block
			.evm_config()
			.evm_with_env(WrapDatabaseRef(db), block.evm_env().clone())
			.transact(&tx)?;

		Ok(ExecutionResult {
			source: Executable::Transaction(tx),
			results: vec![result.result],
			state: result.state,
		})
	}

	fn execute_bundle<'a, DB>(
		_bundle: types::Bundle<P>,
		_block: &BlockContext<P>,
		_db: &'a DB,
	) -> Result<ExecutionResult<P>, types::EvmError<P, ProviderError>>
	where
		DB: DatabaseRef<Error = ProviderError> + Debug + Send + Sync + 'static,
	{
		todo!("executing bundles is not yet implemented");
	}
}

impl<P: Platform> Executable<P> {
	/// Returns all transactions that make up this executable.
	pub fn transactions(&self) -> &[Recovered<types::Transaction<P>>] {
		match self {
			Self::Transaction(tx) => std::slice::from_ref(tx),
			Self::Bundle(bundle) => bundle.transactions(),
		}
	}
}

/// Convinience trait that allows all types that can be executed to be used as a
/// parameter to the `Checkpoint::apply` method.
pub trait IntoExecutable<P: Platform, S = ()> {
	fn try_into_executable(self) -> Result<Executable<P>, RecoveryError>;
}

/// Transactions can be converted into an executable as long as they have a
/// valid recoverable signature.
impl<P: Platform> IntoExecutable<P, ()> for types::Transaction<P> {
	fn try_into_executable(self) -> Result<Executable<P>, RecoveryError> {
		SignedTransaction::try_into_recovered(self)
			.map(Executable::Transaction)
			.map_err(|_| RecoveryError::new())
	}
}

/// Signature recovered individual transactions are always infalliably
/// convertable into an executable.
impl<P: Platform> IntoExecutable<P, u8> for Recovered<types::Transaction<P>> {
	fn try_into_executable(self) -> Result<Executable<P>, RecoveryError> {
		Ok(Executable::Transaction(self))
	}
}

/// Bundles are also convertible into an executable infalliably.
/// Signature recovery is part of the bundle assembly logic.
impl<P: Platform> IntoExecutable<P, u16> for types::Bundle<P> {
	fn try_into_executable(self) -> Result<Executable<P>, RecoveryError> {
		Ok(Executable::Bundle(self))
	}
}

/// This trait represents the overall result of executing a transaction or a
/// bundle of transactions.
///
/// Types implementing this trait provide access to the individual results of
/// transaction executions that make up this overall result.
#[derive(Debug, Clone)]
pub struct ExecutionResult<P: Platform> {
	source: Executable<P>,

	/// For transactions this is guaranteed to be a single-element vector,
	/// for bundles this is guaranteed to be a vector of results for each
	/// transaction in the bundle.
	results: Vec<types::TransactionExecutionResult<P>>,

	/// The aggregated state executing all transactions from the source.
	state: EvmState,
}

impl<P: Platform> ExecutionResult<P> {
	/// Returns the executable that was executed to produce this result.
	pub const fn source(&self) -> &Executable<P> {
		&self.source
	}

	/// Returns the aggregate state changes that were made by executing
	/// the transactions in this execution unit.
	pub const fn state(&self) -> &EvmState {
		&self.state
	}

	/// Access to the individual transaction results that make up this execution
	/// result.
	///
	/// For transactions, this will return a single-element slice containing the
	/// transaction's execution result. For bundles, this will return a slice of
	/// execution results for each transaction in the bundle.
	pub const fn results(&self) -> &[types::TransactionExecutionResult<P>] {
		self.results.as_slice()
	}

	/// Returns individual transactions that were executed as part of this
	/// execution unit.
	pub fn transactions(&self) -> &[Recovered<types::Transaction<P>>] {
		self.source().transactions()
	}

	/// Indicates whether the execution was successful.
	///
	/// For a transaction this means that it was not reverted or halted.
	/// For a bundle this means that bundle requirements around individual
	/// execution results were met.
	pub fn is_success(&self) -> bool {
		match self.source {
			Executable::Transaction(_) => self.results[0].is_success(),
			Executable::Bundle(ref bundle) => {
				// a bundle is successful if all failed transactions are allowed to fail
				for (result, tx) in self.results.iter().zip(bundle.transactions()) {
					if !result.is_success() && !bundle.is_allowed_to_fail(*tx.tx_hash()) {
						return false;
					}
				}

				// if the bundle has a state validity check, run it
				bundle.is_new_state_valid(&self.state)
			}
		}
	}

	/// Returns the cumulative gas used by the execution of this transaction or
	/// bundle.
	pub fn gas_used(&self) -> u64 {
		self.results.iter().map(|r| r.gas_used()).sum()
	}
}
