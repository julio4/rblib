//! Example of defining your own platform types
//!
//! In this example we also show how you can reuse some of the rblib provided
//! platform definitions to build your own platform types with minimal effort.

use {core::convert::Infallible, rblib::*, std::sync::Arc};

#[derive(Debug, Clone, Default)]
struct CustomPlatform;

impl PlatformBase<Optimism> for CustomPlatform {
	type Bundle = CustomBundleType;
	type DefaultLimits = types::DefaultLimits<Optimism>;
	type EvmConfig = types::EvmConfig<Optimism>;
	type NodeTypes = types::NodeTypes<Optimism>;
	type PooledTransaction = types::PooledTransaction<Optimism>;

	fn evm_config(chainspec: Arc<types::ChainSpec<Optimism>>) -> Self::EvmConfig {
		Optimism::evm_config(chainspec)
	}

	fn next_block_environment_context(
		chainspec: &types::ChainSpec<Optimism>,
		parent: &types::Header<Optimism>,
		attributes: &types::PayloadBuilderAttributes<Optimism>,
	) -> types::NextBlockEnvContext<Optimism> {
		Optimism::next_block_environment_context(chainspec, parent, attributes)
	}

	fn build_payload<Provider>(
		payload: Checkpoint<Optimism>,
		provider: &Provider,
	) -> Result<
		types::BuiltPayload<Optimism>,
		reth::payload::builder::PayloadBuilderError,
	>
	where
		Provider: traits::ProviderBounds<Optimism>,
	{
		Optimism::build_payload(payload, provider)
	}
}

#[derive(Debug, Clone, Default)]
struct CustomBundleType;

impl Bundle<Optimism> for CustomBundleType {
	type PostExecutionError = Infallible;

	fn transactions(
		&self,
	) -> &[reth::primitives::Recovered<types::Transaction<Optimism>>] {
		todo!()
	}

	fn without_transaction(self, _: alloy::primitives::TxHash) -> Self {
		todo!()
	}

	fn is_eligible(&self, _: &BlockContext<Optimism>) -> Eligibility {
		Eligibility::Eligible
	}

	fn is_allowed_to_fail(&self, _: alloy::primitives::TxHash) -> bool {
		false
	}

	fn is_optional(&self, _: alloy::primitives::TxHash) -> bool {
		false
	}
}

fn main() {}
