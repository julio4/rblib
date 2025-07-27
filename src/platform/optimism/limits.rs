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
	std::time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Default)]
pub struct OptimismDefaultLimits;

impl<P> LimitsFactory<P> for OptimismDefaultLimits
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
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits {
		let mut limits = Limits::with_gas_limit(
			block
				.attributes()
				.gas_limit
				.unwrap_or_else(|| block.parent().header().gas_limit()),
		)
		.with_deadline(
			Instant::now()
				+ Duration::from_secs(
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
				),
		);

		if let Some(blob_params) = block
			.chainspec()
			.blob_params_at_timestamp(block.attributes().payload_attributes.timestamp)
		{
			limits = limits.with_blob_params(blob_params);
		}

		if let Some(enclosing) = enclosing {
			limits = limits.clamp(enclosing);
		}

		limits
	}
}
