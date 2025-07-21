use {
	super::*,
	crate::{
		reth::{
			ethereum::{evm::EthEvmConfig, node::EthereumNode},
			evm::NextBlockEnvAttributes,
			payload::builder::*,
			primitives::Recovered,
			revm::{cached::CachedReads, cancelled::CancelOnDrop},
			transaction_pool::*,
		},
		traits::*,
	},
	bundle::EthereumBundle,
	limits::EthereumDefaultLimits,
	pool::FixedTransactions,
	reth_transaction_pool::noop::NoopTransactionPool,
	std::sync::Arc,
};

mod bundle;
mod limits;
mod pool;

/// Platform definition for ethereum mainnet.
#[derive(Debug, Clone, Default)]
pub struct Ethereum {
	_private: (),
}

impl Platform for Ethereum {
	type Bundle = EthereumBundle;
	type DefaultLimits = EthereumDefaultLimits;
	type EvmConfig = EthEvmConfig;
	type NodeTypes = EthereumNode;
	type PooledTransaction = EthPooledTransaction;

	fn evm_config(chainspec: Arc<types::ChainSpec<Self>>) -> Self::EvmConfig {
		EthEvmConfig::new(chainspec)
	}

	fn next_block_environment_context(
		_chainspec: &types::ChainSpec<Self>,
		parent: &types::Header<Self>,
		attributes: &types::PayloadBuilderAttributes<Self>,
	) -> types::NextBlockEnvContext<Self> {
		use alloy::consensus::BlockHeader;
		NextBlockEnvAttributes {
			timestamp: attributes.timestamp,
			suggested_fee_recipient: attributes.suggested_fee_recipient,
			prev_randao: attributes.prev_randao,
			gas_limit: EthereumBuilderConfig::new().gas_limit(parent.gas_limit()),
			parent_beacon_block_root: attributes.parent_beacon_block_root,
			withdrawals: Some(attributes.withdrawals.clone()),
		}
	}

	fn construct_payload<Provider>(
		block: &BlockContext<Self>,
		transactions: Vec<Recovered<types::Transaction<Self>>>,
		provider: &Provider,
	) -> Result<types::BuiltPayload<Self>, PayloadBuilderError>
	where
		Provider: ProviderBounds<Self>,
	{
		let evm_config = block.evm_config().clone();
		let payload_config = PayloadConfig {
			parent_header: Arc::new(block.parent().clone()),
			attributes: block.attributes().clone(),
		};

		let build_args = BuildArguments::<
			types::PayloadBuilderAttributes<Self>,
			types::BuiltPayload<Self>,
		>::new(
			CachedReads::default(),
			payload_config,
			CancelOnDrop::default(),
			None,
		);

		let builder_config = EthereumBuilderConfig::new();
		let transactions = Box::new(FixedTransactions::<Self>::new(transactions));

		default_ethereum_payload(
			evm_config,
			provider,
			NoopTransactionPool::default(),
			builder_config,
			build_args,
			|_| {
				transactions
					as Box<
						dyn BestTransactions<
							Item = Arc<ValidPoolTransaction<Self::PooledTransaction>>,
						>,
					>
			},
		)?
		.into_payload()
		.ok_or_else(|| PayloadBuilderError::MissingPayload)
	}
}
