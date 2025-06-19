//! Payload building API
//!
//! This API is used to construct individual payloads.

use reth::{api::{ConfigureEvm, PayloadTypes}, chainspec::EthChainSpec};

mod block;
mod checkpoint;
mod span;

#[cfg(test)]
mod tests;

pub use {block::BlockContext, checkpoint::Checkpoint, span::Span};

/// This type abstracts the platform specific types of the undelying system that
/// is building the payload.
///
/// The payload builder API is agnostic to the underlying payload types, header
/// types, transaction types, and other platform-specific details. It's primary
/// goal is to enable efficient and flexible payload simulation and
/// construction.
///
/// This trait should be customized for every context this API is embedded in.
pub trait Platform: core::fmt::Debug {
	/// The types that are used by the engine API.
	///
	/// Implementations of this trait describe all types that are used by the
	/// consensus engine such as transactions, blocks, headers, etc.
	///
	/// Two well known implementations of this trait are:
	/// - [`EthEngineTypes`] for Ethereum L1 mainnet,
	/// - [`OpEngineTypes`] for Optimism chains.
	type EngineTypes: PayloadTypes;

	/// A type that provides a complete EVM configuration ready to be used
	/// during the payload execution simulation process. This type is used to 
	/// create individual EVM instances that are used to execute transactions.
	type EvmConfig: ConfigureEvm<
		Primitives = types::Primitives<Self>,
	>;

	/// The chain specification type that is used by the platform.
	type ChainSpec: EthChainSpec;

	fn next_block_environment_context(
		chainspec: &Self::ChainSpec,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self>;
}

/// Helpers for extracting types from the platform definition.
pub mod types {
	#![allow(type_alias_bounds)]
	
	use super::*;
	use reth_evm::EvmEnvFor;

	pub(super) type Primitives<P: Platform> = 
		<
			<
				P::EngineTypes as PayloadTypes
			>::BuiltPayload as reth::api::BuiltPayload
		>::Primitives;

	/// Extracts the concrete block type from the platform definition.
	pub type Block<P: Platform> = <Primitives<P> as reth::api::NodePrimitives>::Block;

	/// Extracts the block header type from the platform definition.
	pub type Header<P: Platform> = <Block<P> as reth::api::Block>::Header;

	/// Extracts the type used by FCU when the CL node requests a new payload to
	/// be built by the EL node.
	pub type PayloadBuilderAttributes<P: Platform> =
		<P::EngineTypes as PayloadTypes>::PayloadBuilderAttributes;

	/// Extracts the transaction type for this platform.
	pub type Transaction<P: Platform> = <Primitives<P> as reth::api::NodePrimitives>::SignedTx;

	/// Extracts the type that represents the final outcome of a payload building process,
	/// which is a built payload that can be submitted to the consensus engine.
	pub type BuiltPayload<P: Platform> =
		<P::EngineTypes as PayloadTypes>::BuiltPayload;

	/// Extracts the type that holds chain-specific information needed to build a block
	/// that cannot be deduced automatically from the parent header.
	pub type NextBlockEnvContext<P: Platform> =
		<P::EvmConfig as ConfigureEvm>::NextBlockEnvCtx;

	pub type EvmEnv<P: Platform> = EvmEnvFor<P::EvmConfig>;

	pub type EvmError<P: Platform> = <P::EvmConfig as ConfigureEvm>::Error;
}
