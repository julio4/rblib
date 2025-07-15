use {
	super::{types, *},
	alloy::primitives::Bytes,
	reth_optimism_forks::OpHardforks,
	reth_optimism_node::{OpEvmConfig, OpNextBlockEnvAttributes, OpNode},
	std::sync::Arc,
};
#[cfg(feature = "pipelines")]
use {
	reth::primitives::Recovered,
	reth_optimism_node::txpool::OpPooledTransaction,
};

/// Platform definition for Optimism Rollup chains.
#[derive(Debug, Clone, Default)]
pub struct Optimism;

impl Platform for Optimism {
	#[cfg(feature = "pipelines")]
	type DefaultLimits = OptimismDefaultLimits;
	type EvmConfig = OpEvmConfig;
	type NodeTypes = OpNode;
	#[cfg(feature = "pipelines")]
	type PooledTransaction = OpPooledTransaction;

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
				Bytes::default()
			},
		}
	}

	#[cfg(feature = "pipelines")]
	fn construct_payload<Pool, Provider>(
		block: &BlockContext<Self>,
		transactions: Vec<Recovered<types::Transaction<Self>>>,
		_: &Pool,
		provider: &Provider,
	) -> Result<
		types::BuiltPayload<Self>,
		reth_payload_builder::PayloadBuilderError,
	>
	where
		Pool: traits::PoolBounds<Self>,
		Provider: traits::ProviderBounds<Self>,
	{
		use {
			alloy::eips::Encodable2718,
			op_alloy::consensus::OpPooledTransaction as AlloyPoolTx,
			reth::revm::{cancelled::CancelOnDrop, database::StateProviderDatabase},
			reth_basic_payload_builder::{BuildOutcomeKind, PayloadConfig},
			reth_optimism_node::{
				OpDAConfig,
				payload::builder::{OpBuilder, OpPayloadBuilderCtx},
			},
			reth_payload_util::PayloadTransactionsFixed,
		};

		let transactions = skip_sequencer_transactions(transactions, block);

		let op_builder = OpBuilder::new(|_| {
			PayloadTransactionsFixed::new(
				transactions
					.into_iter()
					.map(|recovered| {
						let encoded_len = recovered.encode_2718_len();
						OpPooledTransaction::<_, AlloyPoolTx>::new(recovered, encoded_len)
					})
					.collect(),
			)
		});

		let context = OpPayloadBuilderCtx {
			evm_config: block.evm_config(),
			da_config: OpDAConfig::default(),
			chain_spec: block.chainspec().clone(),
			config: PayloadConfig::<types::PayloadBuilderAttributes<Self>, _>::new(
				block.parent().clone().into(),
				(*block.attributes()).clone(),
			),
			cancel: CancelOnDrop::default(),
			best_payload: None,
		};

		// Top of Block chain state.
		let state_provider = provider.state_by_block_hash(block.parent().hash())?;

		// Invoke the builder implementation from reth-optimism-node.
		let build_outcome = op_builder.build(
			StateProviderDatabase(&state_provider),
			&state_provider,
			context,
		)?;

		// extract the built payload from the build outcome.
		let built_payload = match build_outcome {
			BuildOutcomeKind::Better { payload }
			| BuildOutcomeKind::Freeze(payload) => payload,
			BuildOutcomeKind::Aborted { .. } => unreachable!(
				"We are not providing the best_payload argument to the builder."
			),
			BuildOutcomeKind::Cancelled => {
				unreachable!("CancelOnDrop is not dropped in this context.")
			}
		};

		// Done!
		Ok(built_payload)
	}
}

#[cfg(feature = "pipelines")]
#[derive(Debug, Clone, Default)]
pub struct OptimismDefaultLimits;

#[cfg(feature = "pipelines")]
impl LimitsFactory<Optimism> for OptimismDefaultLimits {
	fn create(
		&self,
		block: &BlockContext<Optimism>,
		enclosing: Option<&Limits>,
	) -> Limits {
		use {
			alloy::consensus::BlockHeader,
			reth::chainspec::EthChainSpec,
			std::time::Instant,
		};
		let mut limits = Limits::with_gas_limit(
			block
				.attributes()
				.gas_limit
				.unwrap_or_else(|| block.parent().header().gas_limit()),
		)
		.with_deadline(
			Instant::now()
				+ std::time::Duration::from_secs(
					block.attributes().payload_attributes.timestamp
						- std::time::SystemTime::now()
							.duration_since(std::time::UNIX_EPOCH)
							.unwrap_or_default()
							.as_secs(),
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

/// The op builder will automatically inject all transactions that are in the
/// payload attributes from the CL node. We will need to ensure that if those
/// transactions are in the transactions list, they are not duplicated and
/// removed from the transactions list provided as an argument.
///
/// Payload builders might want to explicitly add those transactions during
/// the progressive payload building process to have visibility into the
/// state changes they cause and to know the cumulative gas usage including
/// those txs. This happens for example when a pipeline has a the
/// `OptimismPrologue` step that applies the sequencer transactions to the
/// payload before any other step.
fn skip_sequencer_transactions(
	transactions: Vec<Recovered<types::Transaction<Optimism>>>,
	block: &BlockContext<Optimism>,
) -> Vec<Recovered<types::Transaction<Optimism>>> {
	let sequencer_txs = block
		.attributes()
		.transactions
		.iter()
		.map(|tx| tx.value().tx_hash());

	let mut prefix_len = 0;
	for (ix, sequencer_tx) in sequencer_txs.enumerate() {
		if transactions
			.get(ix)
			.is_some_and(|tx| tx.tx_hash() == sequencer_tx)
		{
			prefix_len += 1;
		}
	}

	transactions
		.into_iter()
		.skip(prefix_len)
		.collect::<Vec<_>>()
}
