use {
	super::*,
	reth::chainspec::ChainSpec,
	reth_ethereum::{evm::EthEvmConfig, node::EthereumNode},
};

#[derive(Debug)]
pub struct Ethereum;

impl Platform for Ethereum {
	type ChainSpec = ChainSpec;
	type EvmConfig = EthEvmConfig;
	type NodeTypes = EthereumNode;

	fn next_block_environment_context(
		chainspec: &Self::ChainSpec,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self> {
		todo!("Ethereum::next_block_environment_context")
	}
}
