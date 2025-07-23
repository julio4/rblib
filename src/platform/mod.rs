//! Platform abstraction layer
//!
//! This module fines the platform specific extension points for the undelying
//! node that executes pipelines. By default rblib provides implementations of
//! the standard Ethereum and Optimism platforms, but it can be extended to
//! support other platforms as well.

use super::*;

mod bundle;
mod ethereum;
mod limits;

pub use {bundle::*, ethereum::Ethereum, limits::*};

#[cfg(feature = "optimism")]
mod optimism;

#[cfg(feature = "optimism")]
pub use optimism::Optimism;

/// This type abstracts the platform specific types of the undelying node that
/// is building or manipulating payloads.
///
/// The payload builder API is agnostic to the underlying payload types, header
/// types, transaction types, and other platform-specific details. It's primary
/// goal is to enable efficient and flexible payload simulation and
/// construction.
///
/// This trait should be customized for every context this API is embedded in.
pub trait PlatformBase<P: PlatformBase<P>>:
	Sized + Clone + core::fmt::Debug + Default + Send + Sync + Unpin + 'static
{
	/// Type that configures the essential types of an Ethereum-like node.
	///
	/// Implementations of this trait describe all types that are used by the
	/// consensus engine such as transactions, blocks, headers, etc.
	///
	/// Two well known implementations of this trait are:
	/// - [`EthereumNode`] for Ethereum L1 mainnet,
	/// - [`OpNode`] for Optimism chains.
	type NodeTypes: reth::api::NodeTypes;

	/// A type that provides a complete EVM configuration ready to be used
	/// during the payload execution simulation process. This type is used to
	/// create individual EVM instances that are used to execute transactions.
	type EvmConfig: reth::api::ConfigureEvm<
			Primitives = types::Primitives<P>,
			NextBlockEnvCtx: Send + Sync + 'static,
		>;

	/// Type that represents transactions that are inside the transaction pool.
	type PooledTransaction: reth::transaction_pool::EthPoolTransaction<
			Consensus = types::Transaction<P>,
		> + Send
		+ Sync
		+ 'static;

	/// Type that configures how bundles are represented and handled by the
	/// platform.
	type Bundle: Bundle<P>;

	/// Type that can provide limits for the payload building process.
	/// If no limits are set on a pipeline, a default instance of this type
	/// will be used.
	type DefaultLimits: LimitsFactory<P> + Default + Send + Sync + 'static;

	/// Instantiate the EVM configuration for the platform with a given chain
	/// specification.
	fn evm_config(
		chainspec: std::sync::Arc<types::ChainSpec<P>>,
	) -> Self::EvmConfig;

	fn next_block_environment_context(
		chainspec: &types::ChainSpec<P>,
		parent: &types::Header<P>,
		attributes: &types::PayloadBuilderAttributes<P>,
	) -> types::NextBlockEnvContext<P>;

	fn build_payload<Provider>(
		payload: Checkpoint<P>,
		provider: &Provider,
	) -> Result<types::BuiltPayload<P>, reth::payload::builder::PayloadBuilderError>
	where
		Provider: traits::ProviderBounds<P>;
}

pub trait Platform: PlatformBase<Self> {}

impl<T> Platform for T where
	T: PlatformBase<T>
		+ Sized
		+ Clone
		+ core::fmt::Debug
		+ Default
		+ Send
		+ Sync
		+ Unpin
		+ 'static
{
}

/// Helpers for extracting types from the platform definition.
pub mod types {
	use {
		super::*,
		reth::revm::context::result::ExecutionResult,
		reth_evm::EvmFactoryFor,
	};

	/// Extracts the type that configures the essential types of the platform.
	pub type NodeTypes<P: Platform> = P::NodeTypes;

	/// Extracts node's engine API types used when interacting with CL.
	pub type PayloadTypes<P: Platform> =
		<NodeTypes<P> as reth::api::NodeTypes>::Payload;

	/// Extracts the node's primitive types that define transactions, blocks,
	/// headers, etc.
	pub type Primitives<P: Platform> =
		<BuiltPayload<P> as reth::api::BuiltPayload>::Primitives;

	/// Extracts the concrete block type from the platform definition.
	pub type Block<P: Platform> =
		<Primitives<P> as reth::api::NodePrimitives>::Block;

	/// Extracts the block header type from the platform definition.
	pub type Header<P: Platform> = <Block<P> as reth::api::Block>::Header;

	/// Extracts the platform-specific payload attributes type that comes from the
	/// CL node.
	pub type PayloadAttributes<P: Platform> =
		<PayloadTypes<P> as reth::api::PayloadTypes>::PayloadAttributes;

	/// Extracts the type used internally during payload building  when the CL
	/// node requests a new payload to be built by the EL node.
	pub type PayloadBuilderAttributes<P: Platform> =
		<PayloadTypes<P> as reth::api::PayloadTypes>::PayloadBuilderAttributes;

	/// Extracts the transaction type for this platform.
	pub type Transaction<P: Platform> =
		<Primitives<P> as reth::api::NodePrimitives>::SignedTx;

	/// Extracts the type that represents a transaction that is in the transaction
	/// pool.
	pub type PooledTransaction<P: Platform> = P::PooledTransaction;

	/// Extracts the type that represents an atomic bundle of transactions.
	pub type Bundle<P: Platform> = P::Bundle;

	/// Extracts the type that represent the optional post-execution bundle
	/// validation error.
	pub type BundlePostExecutionError<P: Platform> =
		<Bundle<P> as bundle::Bundle<P>>::PostExecutionError;

	/// Extracts the type that represents the final outcome of a payload building
	/// process, which is a built payload that can be submitted to the consensus
	/// engine.
	pub type BuiltPayload<P: Platform> =
		<PayloadTypes<P> as reth::api::PayloadTypes>::BuiltPayload;

	/// Extracts the type that holds chain-specific information needed to build a
	/// block that cannot be deduced automatically from the parent header.
	pub type NextBlockEnvContext<P: Platform> =
		<EvmConfig<P> as reth::api::ConfigureEvm>::NextBlockEnvCtx;

	/// Extracts the type that represents the EVM environment for this platform.
	pub type EvmEnv<P: Platform> = reth::evm::EvmEnvFor<EvmConfig<P>>;

	/// Extracts the type that holds the EVM configuration for this platform.
	pub type EvmConfig<P: Platform> = P::EvmConfig;

	/// Extracts the error type used by the EVM environment configuration when
	/// creating evm environment for a new block. See
	/// [`reth::api::ConfigureEvm::next_evm_env`].
	pub type EvmEnvError<P: Platform> =
		<EvmConfig<P> as reth::api::ConfigureEvm>::Error;

	/// Extracts the `ChainSpec` type from the platform definition.
	pub type ChainSpec<P: Platform> =
		<P::NodeTypes as reth::api::NodeTypes>::ChainSpec;

	/// Extracts the type that is responsible for creating instances of the EVM
	/// for this platform.
	pub type EvmFactory<P: Platform> = EvmFactoryFor<P::EvmConfig>;

	/// Extracts the type returned by EVM. Contains either errors related to
	/// invalid transactions or internal irrecoverable execution errors.
	pub type EvmError<P: Platform, DBError> =
		<EvmFactory<P> as reth::evm::EvmFactory>::Error<DBError>;

	/// Extracts the type that describes the reason why EVM execution of a
	/// transaction was halted.
	pub type EvmHaltReason<P: Platform> =
		<EvmFactory<P> as reth::evm::EvmFactory>::HaltReason;

	/// Extracts the type that can provide limits for the payload building
	/// process.
	pub type DefaultLimits<P: Platform> = P::DefaultLimits;

	/// The result of executing a transaction in the EVM.
	pub type TransactionExecutionResult<P: Platform> =
		ExecutionResult<EvmHaltReason<P>>;
}
