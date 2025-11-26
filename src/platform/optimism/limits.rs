use {
	crate::{
		alloy::consensus::BlockHeader,
		prelude::*,
		reth::{
			api::NodeTypes,
			chainspec::EthChainSpec,
			node::builder::PayloadTypes,
			optimism::node::OpPayloadBuilderAttributes,
		},
	},
	core::time::Duration,
	std::time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Default)]
pub struct OptimismDefaultLimits;

#[allow(clippy::struct_field_names)]
#[derive(Default, Debug, Clone, Copy)]
pub struct OpLimitsExt {
	pub max_tx_da: Option<u64>,
	pub max_block_da: Option<u64>,
	pub max_block_da_footprint: Option<u64>,
}

impl LimitExtension for OpLimitsExt {
	fn clamp(&self, other: &Self) -> Self {
		Self {
			max_tx_da: self.max_tx_da.min(other.max_tx_da),
			max_block_da: self.max_block_da.min(other.max_block_da),
			max_block_da_footprint: self
				.max_block_da_footprint
				.min(other.max_block_da_footprint),
		}
	}
}

impl<P> PlatformLimits<P> for OptimismDefaultLimits
where
	P: Platform<
		NodeTypes: NodeTypes<
			Payload: PayloadTypes<
				PayloadBuilderAttributes = OpPayloadBuilderAttributes<
					types::Transaction<P>,
				>,
			>,
		>,
	>,
{
	fn create(&self, block: &BlockContext<P>) -> Limits<P> {
		let mut limits = Limits::gas_limit(
			block
				.attributes()
				.gas_limit
				.unwrap_or_else(|| block.parent().header().gas_limit()),
		)
		.with_deadline(Duration::from_secs(
			block
				.attributes()
				.payload_attributes
				.timestamp
				.saturating_sub(
					SystemTime::now()
						.duration_since(UNIX_EPOCH)
						.unwrap_or_default()
						.as_secs(),
				),
		));

		if let Some(blob_params) = block
			.chainspec()
			.blob_params_at_timestamp(block.attributes().payload_attributes.timestamp)
		{
			limits = limits.with_blob_params(blob_params);
		}

		limits
	}
}
