// Public API re-exports
mod payload;
pub use payload::*;

#[cfg(feature = "pipelines")]
mod pipelines;

#[cfg(feature = "pipelines")]
pub use pipelines::*;

#[cfg(feature = "optimism")]
mod optimism;

#[cfg(feature = "optimism")]
pub use optimism::Optimism;

#[cfg(feature = "ethereum")]
mod ethereum;

#[cfg(feature = "ethereum")]
pub use ethereum::Ethereum;

/// This type abstracts the platform specific types of the undelying system that
/// is building the payload.
///
/// The payload builder API is agnostic to the underlying payload types, header
/// types, transaction types, and other platform-specific details. It's primary
/// goal is to enable efficient and flexible payload simulation and
/// construction.
///
/// This trait should be customized for every context this API is embedded in.
pub trait Platform:
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
			Primitives = types::Primitives<Self>,
			NextBlockEnvCtx: Send + Sync + 'static,
		>;

	/// Type that represents transactions that are inside the transaction pool.
	#[cfg(feature = "pipelines")]
	type PooledTransaction: reth_transaction_pool::EthPoolTransaction<
			Consensus = types::Transaction<Self>,
		> + Send
		+ Sync
		+ 'static;

	/// Type that can provide limits for the payload building process.
	/// If no limits are set on a pipeline, a default instance of this type
	/// will be used.
	#[cfg(feature = "pipelines")]
	type DefaultLimits: LimitsFactory<Self> + Default + Send + Sync + 'static;

	/// Instantiate the EVM configuration for the platform with a given chain
	/// specification.
	fn evm_config(
		chainspec: std::sync::Arc<types::ChainSpec<Self>>,
	) -> Self::EvmConfig;

	fn next_block_environment_context(
		chainspec: &types::ChainSpec<Self>,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self>;

	#[cfg(feature = "pipelines")]
	fn construct_payload<Pool, Provider>(
		block: &BlockContext<Self>,
		transactions: Vec<reth::primitives::Recovered<types::Transaction<Self>>>,
		transaction_pool: &Pool,
		provider: &Provider,
	) -> Result<
		types::BuiltPayload<Self>,
		reth_payload_builder::PayloadBuilderError,
	>
	where
		Pool: traits::PoolBounds<Self>,
		Provider: traits::ProviderBounds<Self>;
}

/// Helpers for extracting types from the platform definition.
pub mod types {
	use super::Platform;

	/// Extracts node's engine API types used when interacting with CL.
	pub type NodePayloadTypes<P: Platform> =
		<P::NodeTypes as reth::api::NodeTypes>::Payload;

	/// Extracts the node's primitive types that define transactions, blocks,
	/// headers, etc.
	pub type Primitives<P: Platform> =
		<
			<
				NodePayloadTypes<P> as reth::api::PayloadTypes
			>::BuiltPayload as reth::api::BuiltPayload
		>::Primitives;

	/// Extracts the concrete block type from the platform definition.
	pub type Block<P: Platform> =
		<Primitives<P> as reth::api::NodePrimitives>::Block;

	/// Extracts the block header type from the platform definition.
	pub type Header<P: Platform> = <Block<P> as reth::api::Block>::Header;

	/// Extracts the platform-specific payload attributes type that comes from the
	/// CL node.
	pub type PayloadAttributes<P: Platform> =
		<NodePayloadTypes<P> as reth::api::PayloadTypes>::PayloadAttributes;

	/// Extracts the type used internally during payload building  when the CL
	/// node requests a new payload to be built by the EL node.
	pub type PayloadBuilderAttributes<P: Platform> =
		<NodePayloadTypes<P> as reth::api::PayloadTypes>::PayloadBuilderAttributes;

	/// Extracts the transaction type for this platform.
	pub type Transaction<P: Platform> =
		<Primitives<P> as reth::api::NodePrimitives>::SignedTx;

	/// Extracts the type that represents the final outcome of a payload building
	/// process, which is a built payload that can be submitted to the consensus
	/// engine.
	pub type BuiltPayload<P: Platform> =
		<NodePayloadTypes<P> as reth::api::PayloadTypes>::BuiltPayload;

	/// Extracts the type that holds chain-specific information needed to build a
	/// block that cannot be deduced automatically from the parent header.
	pub type NextBlockEnvContext<P: Platform> =
		<P::EvmConfig as reth::api::ConfigureEvm>::NextBlockEnvCtx;

	/// Extracts the type that represents the EVM environment for this platform.
	pub type EvmEnv<P: Platform> = reth_evm::EvmEnvFor<P::EvmConfig>;

	/// Extracts the error type used by the EVM configuration for this platform.
	pub type EvmError<P: Platform> =
		<P::EvmConfig as reth::api::ConfigureEvm>::Error;

	/// Extracts the `ChainSpec` type from the platform definition.
	pub type ChainSpec<P: Platform> =
		<P::NodeTypes as reth::api::NodeTypes>::ChainSpec;
}
