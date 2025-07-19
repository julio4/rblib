use {
	crate::*,
	alloy::consensus::crypto::RecoveryError,
	reth::{
		ethereum::primitives::SignedTransaction,
		evm::revm::state::EvmState,
		primitives::Recovered,
	},
	std::fmt::Debug,
};

/// Describes a unit of execution that can be used to create a state
/// checkpoint.
pub enum Executable<P: Platform> {
	// Individual transaction
	Transaction(Recovered<types::Transaction<P>>),
	// A bundle of transactions with its metadata
	Bundle(types::Bundle<P>),
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
pub enum ExecutionResult<'a, P: Platform> {
	Barrier,
	Transaction(&'a types::TransactionExecutionResult<P>),
	Bundle(
		&'a types::Bundle<P>,
		&'a Vec<types::TransactionExecutionResult<P>>,
		&'a EvmState,
	),
}

impl<'a, P: Platform> ExecutionResult<'a, P> {
	/// Indicates whether the execution was successful.
	///
	/// For a transaction this means that it was not reverted or halted.
	/// For a bundle this means that bundle requirements around individual
	/// execution results were met.
	pub fn is_success(&self) -> bool {
		match self {
			Self::Barrier => true,
			Self::Transaction(result) => result.is_success(),
			Self::Bundle(bundle, results, state) => {
				// a bundle is successful if all failed transactions are allowed to fail
				for (result, tx) in results.iter().zip(bundle.transactions()) {
					if !result.is_success() && !bundle.is_allowed_to_fail(*tx.tx_hash()) {
						return false;
					}
				}

				// if the bundle has a state validity check, run it
				bundle.is_new_state_valid(state)
			}
		}
	}

	/// Returns the cumulative gas used by the execution of this transaction or
	/// bundle.
	pub fn gas_used(&self) -> u64 {
		match self {
			Self::Barrier => 0,
			Self::Transaction(result) => result.gas_used(),
			Self::Bundle(_, results, _) => results.iter().map(|r| r.gas_used()).sum(),
		}
	}

	/// Access to the individual transaction results that make up this execution
	/// result.
	///
	/// For transactions, this will return a single-element slice containing the
	/// transaction's execution result. For bundles, this will return a slice of
	/// execution results for each transaction in the bundle.
	pub fn results(&self) -> &'a [types::TransactionExecutionResult<P>] {
		match self {
			Self::Barrier => &[],
			Self::Transaction(result) => std::slice::from_ref(result),
			Self::Bundle(_, results, _) => results,
		}
	}
}
