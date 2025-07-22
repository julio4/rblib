//! Extensions for creating mocked versions of various API primitives that
//! typically have non-trivial construction process.

use {
	super::*,
	alloy::primitives::{Address, B256},
	reth::{
		chainspec::DEV,
		ethereum::node::engine::EthPayloadAttributes as PayloadAttributes,
		payload::builder::EthPayloadBuilderAttributes,
		primitives::SealedHeader,
		providers::test_utils::MockEthProvider,
	},
};

/// Payload builder attributes typically come from the Consensus Layer with a
/// forkchoiceUpdate request, and are typically obtained from instances of types
/// that implement [`PayloadJobGenerator`] and are configured as payload
/// builders.
///
/// See [`crate::pipelines::service::PipelineServiceBuilder`] for an example of
/// a workflow that creates a payload builder attributes instance in real world
/// settings.
pub trait PayloadBuilderAttributesMocked {
	fn mocked(parent: &SealedHeader) -> Self;
}

impl PayloadBuilderAttributesMocked for EthPayloadBuilderAttributes {
	fn mocked(parent: &SealedHeader) -> Self {
		EthPayloadBuilderAttributes::new(parent.hash(), PayloadAttributes {
			timestamp: parent.header().timestamp + 1,
			prev_randao: B256::random(),
			suggested_fee_recipient: Address::random(),
			withdrawals: Some(vec![]),
			parent_beacon_block_root: Some(B256::ZERO),
		})
	}
}

#[cfg(feature = "optimism")]
impl PayloadBuilderAttributesMocked
	for reth::optimism::node::OpPayloadBuilderAttributes<
		types::Transaction<Optimism>,
	>
{
	fn mocked(parent: &SealedHeader) -> Self {
		use {
			alloy::eips::eip2718::Decodable2718,
			reth::optimism::{
				chainspec::constants::{
					BASE_MAINNET_MAX_GAS_LIMIT,
					TX_SET_L1_BLOCK_OP_MAINNET_BLOCK_124665056,
				},
				node::OpPayloadBuilderAttributes,
			},
			reth_ethereum::primitives::WithEncoded,
		};

		let sequencer_tx = WithEncoded::new(
			TX_SET_L1_BLOCK_OP_MAINNET_BLOCK_124665056.into(),
			types::Transaction::<Optimism>::decode_2718(
				&mut &TX_SET_L1_BLOCK_OP_MAINNET_BLOCK_124665056[..],
			)
			.expect("Failed to decode test transaction"),
		);

		OpPayloadBuilderAttributes {
			payload_attributes: EthPayloadBuilderAttributes::mocked(parent),
			transactions: vec![sequencer_tx],
			gas_limit: Some(BASE_MAINNET_MAX_GAS_LIMIT),
			..Default::default()
		}
	}
}

/// Allows the creation of a block context for the first block post genesis with
/// a all [`FundedAccounts`] pre-funded with 100 ETH.
pub trait BlockContextMocked<P: Platform> {
	/// Returns a tuple of:
	/// 1. an instance of a [`BlockContext`] that is rooted at the genesis block
	///    the given platform's DEV chainspec.
	/// 2. An instance of a platform-specific [`StateProviderFactory`] that has
	///    its state pre-populated with [`FundedAccounts`] for the given platform.
	///    This state provider together with a checkpoint created on top of the
	///    returned [`BlockContext`] can be used to construct a payload using
	///    [`Platform::build_payload`].
	fn mocked() -> (BlockContext<P>, impl traits::ProviderBounds<P>);
}

impl BlockContextMocked<Ethereum> for BlockContext<Ethereum> {
	fn mocked() -> (
		BlockContext<Ethereum>,
		impl traits::ProviderBounds<Ethereum>,
	) {
		let chainspec = DEV.clone();
		let parent = chainspec.genesis_header.clone();
		let state_provider = MockEthProvider::default().with_funded_accounts();
		let payload_attribs = EthPayloadBuilderAttributes::mocked(&parent);

		let block = BlockContext::<Ethereum>::new(
			parent,
			payload_attribs,
			Box::new(state_provider.clone()),
			chainspec,
		)
		.expect("Failed to create mocked block context");

		(block, state_provider)
	}
}

#[cfg(feature = "optimism")]
impl BlockContextMocked<Optimism> for BlockContext<Optimism> {
	fn mocked() -> (
		BlockContext<Optimism>,
		impl traits::ProviderBounds<Optimism>,
	) {
		use reth_optimism_node::OpPayloadBuilderAttributes;

		let chainspec = reth::optimism::chainspec::OP_DEV.clone();
		let parent = chainspec.genesis_header.clone();
		let state_provider = MockEthProvider::default()
			.with_chain_spec(chainspec.as_ref().clone())
			.with_funded_accounts();
		let payload_attributes = OpPayloadBuilderAttributes::mocked(&parent);

		let block = BlockContext::<Optimism>::new(
			parent,
			payload_attributes,
			Box::new(state_provider.clone()),
			chainspec,
		)
		.expect("Failed to create mocked block context");

		(block, state_provider)
	}
}
