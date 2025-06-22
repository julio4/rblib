use {
	super::*,
	crate::traits::{PoolBounds, ProviderBounds},
	alloc::{boxed::Box, sync::Arc},
	core::marker::PhantomData,
	reth_basic_payload_builder::{BuildArguments, PayloadConfig},
	reth_ethereum::{evm::EthEvmConfig, node::EthereumNode},
	reth_ethereum_payload_builder::{
		default_ethereum_payload,
		EthereumBuilderConfig,
	},
	reth_evm::NextBlockEnvAttributes,
	reth_payload_builder::PayloadBuilderError,
	reth_transaction_pool::{
		identifier::TransactionId,
		BestTransactions,
		PoolTransaction,
		TransactionOrigin,
		TransactionPool,
		ValidPoolTransaction,
	},
};

/// Platform definition for ethereum mainnet.
#[derive(Debug)]
pub struct EthereumMainnet;

impl Platform for EthereumMainnet {
	type EvmConfig = EthEvmConfig;
	type NodeTypes = EthereumNode;

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

	fn into_built_payload<Pool, Provider>(
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
		let transactions: alloc::vec::Vec<_> =
			checkpoint.history().transactions().cloned().collect();
		let transactions = Box::new(PreselectedBestTransactions::<Self, Pool>(
			transactions,
			PhantomData,
		));

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

struct PreselectedBestTransactions<Plat, Pool>(
	alloc::vec::Vec<types::Transaction<Plat>>,
	PhantomData<Pool>,
)
where
	Plat: Platform,
	Pool: PoolBounds<Plat>;

impl<Plat, Pool> BestTransactions for PreselectedBestTransactions<Plat, Pool>
where
	Plat: Platform,
	Pool: PoolBounds<Plat>,
{
	fn no_updates(&mut self) {
		todo!()
	}

	fn set_skip_blobs(&mut self, skip_blobs: bool) {
		todo!()
	}

	fn mark_invalid(
		&mut self,
		transaction: &Self::Item,
		kind: reth_transaction_pool::error::InvalidPoolTransactionError,
	) {
		todo!()
	}
}

impl<Plat, Pool> Iterator for PreselectedBestTransactions<Plat, Pool>
where
	Plat: Platform,
	Pool: PoolBounds<Plat>,
{
	type Item = Arc<ValidPoolTransaction<<Pool as TransactionPool>::Transaction>>;

	fn next(&mut self) -> Option<Self::Item> {
		let transaction = self.0.pop()?;
		use reth_ethereum::primitives::SignerRecoverable;

		let Ok(pooled) = <<Pool as TransactionPool>::Transaction as PoolTransaction>::try_from_consensus(
			transaction.try_into_recovered().expect("Transaction should be valid at this point"),
		)else {
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
