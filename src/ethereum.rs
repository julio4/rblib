use {
	super::*,
	crate::traits::{PoolBounds, ProviderBounds},
	reth_basic_payload_builder::{BuildArguments, PayloadConfig},
	reth_ethereum::{
		evm::EthEvmConfig,
		node::EthereumNode,
		primitives::SignerRecoverable,
	},
	reth_ethereum_payload_builder::{
		default_ethereum_payload,
		EthereumBuilderConfig,
	},
	reth_evm::NextBlockEnvAttributes,
	reth_payload_builder::PayloadBuilderError,
	reth_transaction_pool::{
		error::InvalidPoolTransactionError,
		identifier::TransactionId,
		BestTransactions,
		EthPooledTransaction,
		PoolTransaction,
		TransactionOrigin,
		ValidPoolTransaction,
	},
	std::sync::Arc,
};

/// Platform definition for ethereum mainnet.
#[derive(Debug, Clone, Default)]
pub struct EthereumMainnet;

impl Platform for EthereumMainnet {
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
		NextBlockEnvAttributes {
			timestamp: attributes.timestamp,
			suggested_fee_recipient: attributes.suggested_fee_recipient,
			prev_randao: attributes.prev_randao,
			gas_limit: EthereumBuilderConfig::new().gas_limit(parent.gas_limit),
			parent_beacon_block_root: attributes.parent_beacon_block_root,
			withdrawals: Some(attributes.withdrawals.clone()),
		}
	}

	fn construct_payload<Pool, Provider>(
		checkpoint: payload::Checkpoint<Self>,
		transaction_pool: &Pool,
		provider: &Provider,
	) -> Result<
		types::BuiltPayload<Self>,
		reth_payload_builder::PayloadBuilderError,
	>
	where
		Pool: PoolBounds<Self>,
		Provider: ProviderBounds<Self>,
	{
		let evm_config = checkpoint.block().evm_config().clone();
		let payload_config = PayloadConfig {
			parent_header: Arc::new(checkpoint.block().parent().clone()),
			attributes: checkpoint.block().attributes().clone(),
		};

		let build_args = BuildArguments::<_, types::BuiltPayload<Self>>::new(
			Default::default(),
			payload_config,
			Default::default(),
			None,
		);

		let builder_config = EthereumBuilderConfig::new();
		let transactions: Vec<_> =
			checkpoint.history().transactions().cloned().collect();
		let transactions =
			Box::new(PreselectedBestTransactions::<Self>(transactions));

		default_ethereum_payload(
			evm_config,
			provider,
			transaction_pool,
			builder_config,
			build_args,
			|_| transactions,
		)?
		.into_payload()
		.ok_or_else(|| PayloadBuilderError::MissingPayload)
	}
}

struct PreselectedBestTransactions<P: Platform>(Vec<types::Transaction<P>>);
impl<P: Platform> BestTransactions for PreselectedBestTransactions<P> {
	fn no_updates(&mut self) {}

	fn set_skip_blobs(&mut self, _: bool) {}

	fn mark_invalid(&mut self, _: &Self::Item, _: InvalidPoolTransactionError) {}
}

impl<P: Platform> Iterator for PreselectedBestTransactions<P> {
	type Item = Arc<ValidPoolTransaction<P::PooledTransaction>>;

	fn next(&mut self) -> Option<Self::Item> {
		let transaction = self.0.pop()?;

		let Ok(pooled) = P::PooledTransaction::try_from_consensus(
			transaction
				.try_into_recovered()
				.expect("Transaction should be valid at this point"),
		) else {
			unreachable!("Transaction should be valid at this point");
		};

		let wrapper = ValidPoolTransaction {
			transaction: pooled,
			transaction_id: TransactionId::new(0.into(), 0),
			propagate: false,
			timestamp: std::time::Instant::now(),
			origin: TransactionOrigin::Private,
			authority_ids: None,
		};

		Some(Arc::new(wrapper))
	}
}
