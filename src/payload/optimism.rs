use {
	super::{platform::types, *},
	reth_optimism_chainspec::OpChainSpec,
	reth_optimism_forks::OpHardforks,
	reth_optimism_node::{OpEngineTypes, OpEvmConfig, OpNextBlockEnvAttributes},
};

#[derive(Debug)]
pub struct Optimism;

impl Platform for Optimism {
	type ChainSpec = OpChainSpec;
	type EngineTypes = OpEngineTypes;
	type EvmConfig = OpEvmConfig;

	fn next_block_environment_context(
		chainspec: &Self::ChainSpec,
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
