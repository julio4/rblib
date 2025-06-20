use {
	super::*,
	alloc::sync::Arc,
	reth_ethereum::{evm::EthEvmConfig, node::EthereumNode},
	reth_ethereum_payload_builder::EthereumBuilderConfig,
	reth_evm::NextBlockEnvAttributes,
};

/// Platform definition for ethereum mainnet.
#[derive(Debug)]
pub struct EthereumMainnet;

impl Platform for EthereumMainnet {
	type EvmConfig = EthEvmConfig;
	type NodeTypes = EthereumNode;

	fn evm_config(chainspec: Arc<types::ChainSpec<Self>>) -> Self::EvmConfig {
		EthEvmConfig::new(chainspec)
	}

	fn next_block_environment_context(
		_chainspec: &types::ChainSpec<Self>,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self> {
		NextBlockEnvAttributes {
			timestamp: attributes.timestamp,
			suggested_fee_recipient: attributes.suggested_fee_recipient,
			prev_randao: attributes.prev_randao,
			gas_limit: EthereumBuilderConfig::new().gas_limit(parent.gas_limit),
			parent_beacon_block_root: attributes.parent_beacon_block_root,
			withdrawals: Some(attributes.withdrawals.clone()),
		}
	}
}
