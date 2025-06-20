#![cfg_attr(not(test), no_std)]

extern crate alloc;

use reth::{
	api::{ConfigureEvm, NodeTypes, PayloadTypes},
	chainspec::EthChainSpec,
};

use alloc::sync::Arc;

mod payload;
mod pipelines;

#[cfg(feature = "optimism")]
mod optimism;

#[cfg(feature = "optimism")]
pub use optimism::Optimism;

#[cfg(feature = "ethereum")]
mod ethereum;

#[cfg(feature = "ethereum")]
pub use ethereum::EthereumMainnet;

// Public API re-exports
pub use {payload::*, pipelines::*};

/// This type abstracts the platform specific types of the undelying system that
/// is building the payload.
///
/// The payload builder API is agnostic to the underlying payload types, header
/// types, transaction types, and other platform-specific details. It's primary
/// goal is to enable efficient and flexible payload simulation and
/// construction.
///
/// This trait should be customized for every context this API is embedded in.
pub trait Platform: core::fmt::Debug + Send + Sync + Unpin + 'static {
	/// Type that configures the essential types of an Ethereum-like node.
	///
	/// Implementations of this trait describe all types that are used by the
	/// consensus engine such as transactions, blocks, headers, etc.
	///
	/// Two well known implementations of this trait are:
	/// - [`EthereumNode`] for Ethereum L1 mainnet,
	/// - [`OpNode`] for Optimism chains.
	type NodeTypes: NodeTypes;

	/// A type that provides a complete EVM configuration ready to be used
	/// during the payload execution simulation process. This type is used to 
	/// create individual EVM instances that are used to execute transactions.
	type EvmConfig: ConfigureEvm<
		Primitives = types::Primitives<Self>,
	>;

	fn evm_config(chainspec: Arc<types::ChainSpec<Self>>) -> Self::EvmConfig;

	fn next_block_environment_context(
		chainspec: &types::ChainSpec<Self>,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self>;
}

/// Helpers for extracting types from the platform definition.
pub mod types {
	#![allow(type_alias_bounds)]
	
	use super::*;
	use reth_evm::EvmEnvFor;

  /// Extracts node's engine API types used when interacting with CL.
  pub type NodePayloadTypes<P: Platform> = <P::NodeTypes as NodeTypes>::Payload;

  /// Extracts the node's primitive types that define transactions, blocks, headers, etc.
	pub type Primitives<P: Platform> = 
		<
			<
				NodePayloadTypes<P> as PayloadTypes
			>::BuiltPayload as reth::api::BuiltPayload
		>::Primitives;


	/// Extracts the concrete block type from the platform definition.
	pub type Block<P: Platform> = <Primitives<P> as reth::api::NodePrimitives>::Block;

	/// Extracts the block header type from the platform definition.
	pub type Header<P: Platform> = <Block<P> as reth::api::Block>::Header;

	/// Extracts the type used by FCU when the CL node requests a new payload to
	/// be built by the EL node.
	pub type PayloadBuilderAttributes<P: Platform> =
		<NodePayloadTypes<P> as PayloadTypes>::PayloadBuilderAttributes;

	/// Extracts the transaction type for this platform.
	pub type Transaction<P: Platform> = <Primitives<P> as reth::api::NodePrimitives>::SignedTx;

	/// Extracts the type that represents the final outcome of a payload building process,
	/// which is a built payload that can be submitted to the consensus engine.
	pub type BuiltPayload<P: Platform> =
		<NodePayloadTypes<P> as PayloadTypes>::BuiltPayload;

	/// Extracts the type that holds chain-specific information needed to build a block
	/// that cannot be deduced automatically from the parent header.
	pub type NextBlockEnvContext<P: Platform> =
		<P::EvmConfig as ConfigureEvm>::NextBlockEnvCtx;

	/// Extracts the type that represents the EVM environment for this platform.
	pub type EvmEnv<P: Platform> = EvmEnvFor<P::EvmConfig>;

	/// Extracts the error type used by the EVM configuration for this platform.
	pub type EvmError<P: Platform> = <P::EvmConfig as ConfigureEvm>::Error;

	/// Extracts the ChainSpec type from the platform definition.
	pub type ChainSpec<P: Platform> = <P::NodeTypes as NodeTypes>::ChainSpec;
}
