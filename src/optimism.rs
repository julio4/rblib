use {
	super::{types, *},
	reth_optimism_forks::OpHardforks,
	reth_optimism_node::{OpEvmConfig, OpNextBlockEnvAttributes, OpNode},
};

/// Platform definition for Optimism Rollup chains.
#[derive(Debug)]
pub struct Optimism;

impl Platform for Optimism {
	type EvmConfig = OpEvmConfig;
	type NodeTypes = OpNode;

	fn evm_config(chainspec: Arc<types::ChainSpec<Self>>) -> Self::EvmConfig {
		OpEvmConfig::optimism(chainspec)
	}

	fn next_block_environment_context(
		chainspec: &types::ChainSpec<Self>,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self> {
		OpNextBlockEnvAttributes {
			timestamp: attributes.payload_attributes.timestamp,
			suggested_fee_recipient: attributes
				.payload_attributes
				.suggested_fee_recipient,
			prev_randao: attributes.payload_attributes.prev_randao,
			gas_limit: attributes.gas_limit.unwrap_or(parent.gas_limit),
			parent_beacon_block_root: attributes
				.payload_attributes
				.parent_beacon_block_root,
			extra_data: if chainspec.is_holocene_active_at_timestamp(
				attributes.payload_attributes.timestamp,
			) {
				attributes
					.get_holocene_extra_data(chainspec.base_fee_params_at_timestamp(
						attributes.payload_attributes.timestamp,
					))
					.unwrap_or_default()
			} else {
				Default::default()
			},
		}
	}
}
