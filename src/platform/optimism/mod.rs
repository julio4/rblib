use {
	super::{
		alloy::{
			eips::Encodable2718,
			optimism::consensus::OpPooledTransaction as AlloyPoolTx,
			primitives::Bytes,
		},
		reth::{
			chainspec::EthChainSpec,
			optimism::{
				forks::OpHardforks,
				node::{payload::builder::*, txpool::OpPooledTransaction, *},
			},
			payload::builder::*,
			primitives::Recovered,
			revm::{cancelled::CancelOnDrop, database::StateProviderDatabase},
		},
		types,
		*,
	},
	bundle::OpBundle,
	limits::OptimismDefaultLimits,
	reth_payload_util::PayloadTransactionsFixed,
	std::sync::Arc,
};

mod bundle;
mod limits;

/// Platform definition for Optimism Rollup chains.
#[derive(Debug, Clone, Default)]
pub struct Optimism;

impl Platform for Optimism {
	type Bundle = OpBundle;
	type DefaultLimits = OptimismDefaultLimits;
	type EvmConfig = OpEvmConfig;
	type NodeTypes = OpNode;
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

	fn construct_payload<Pool, Provider>(
		block: &BlockContext<Self>,
		transactions: Vec<Recovered<types::Transaction<Self>>>,
		_: &Pool,
		provider: &Provider,
	) -> Result<types::BuiltPayload<Self>, PayloadBuilderError>
	where
		Pool: traits::PoolBounds<Self>,
		Provider: traits::ProviderBounds<Self>,
	{
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
