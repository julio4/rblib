use {
	super::*,
	crate::traits::{PoolBounds, ProviderBounds},
	alloy::consensus::Transaction,
	reth::{
		chainspec::EthChainSpec,
		payload::PayloadBuilderAttributes,
		primitives::Recovered,
		revm::{cached::CachedReads, cancelled::CancelOnDrop},
	},
	reth_basic_payload_builder::{BuildArguments, PayloadConfig},
	reth_ethereum::{evm::EthEvmConfig, node::EthereumNode},
	reth_ethereum_payload_builder::{
		EthereumBuilderConfig,
		default_ethereum_payload,
	},
	reth_evm::NextBlockEnvAttributes,
	reth_payload_builder::PayloadBuilderError,
	reth_transaction_pool::{
		BestTransactions,
		EthPooledTransaction,
		PoolTransaction,
		TransactionOrigin,
		ValidPoolTransaction,
		error::InvalidPoolTransactionError,
		identifier::{SenderId, SenderIdentifiers, TransactionId},
	},
	std::{
		collections::{HashMap, hash_map::Entry},
		sync::Arc,
	},
};

/// Platform definition for ethereum mainnet.
#[derive(Debug, Clone, Default)]
pub struct Ethereum;

impl Platform for Ethereum {
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

	fn construct_payload<Pool, Provider>(
		block: &BlockContext<Self>,
		transactions: Vec<Recovered<types::Transaction<Self>>>,
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
		let evm_config = block.evm_config().clone();
		let payload_config = PayloadConfig {
			parent_header: Arc::new(block.parent().clone()),
			attributes: block.attributes().clone(),
		};

		let build_args = BuildArguments::<_, types::BuiltPayload<Self>>::new(
			CachedReads::default(),
			payload_config,
			CancelOnDrop::default(),
			None,
		);

		let builder_config = EthereumBuilderConfig::new();
		let transactions =
			Box::new(PreselectedBestTransactions::<Self>::new(transactions));

		default_ethereum_payload(
			evm_config,
			provider,
			transaction_pool,
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

#[derive(Debug, Clone, Default)]
pub struct EthereumDefaultLimits(EthereumBuilderConfig);

impl EthereumDefaultLimits {
	pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
		self.0 = self.0.with_gas_limit(gas_limit);
		self
	}
}

impl<P: Platform> LimitsFactory<P> for EthereumDefaultLimits {
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits {
		use alloy::consensus::BlockHeader;
		let timestamp = block.attributes().timestamp();
		let parent_gas_limit = block.parent().gas_limit();
		let mut gas_limit = self.0.gas_limit(parent_gas_limit);

		if let Some(enclosing) = enclosing {
			gas_limit = gas_limit.min(enclosing.gas_limit);
		}

		let limits = Limits::with_gas_limit(gas_limit);

		if let Some(mut blob_params) =
			block.chainspec().blob_params_at_timestamp(timestamp)
		{
			if let Some(enclosing) = enclosing.and_then(|e| e.blob_params) {
				blob_params.target_blob_count = enclosing
					.target_blob_count
					.min(blob_params.target_blob_count);

				blob_params.max_blob_count =
					enclosing.max_blob_count.min(blob_params.max_blob_count);
			}

			return limits.with_blob_params(blob_params);
		}

		limits
	}
}

struct PreselectedBestTransactions<P: Platform> {
	txs: Vec<Recovered<types::Transaction<P>>>,
	senders: SenderIdentifiers,
	invalid: HashMap<SenderId, TransactionId>,
}

impl<P: Platform> PreselectedBestTransactions<P> {
	pub fn new(txs: Vec<Recovered<types::Transaction<P>>>) -> Self {
		// reverse because we want to pop from the end
		// in the iterator.
		let mut txs = txs;
		txs.reverse();

		Self {
			txs,
			senders: SenderIdentifiers::default(),
			invalid: HashMap::new(),
		}
	}
}

impl<P: Platform> BestTransactions for PreselectedBestTransactions<P> {
	fn no_updates(&mut self) {}

	fn set_skip_blobs(&mut self, _: bool) {}

	fn mark_invalid(&mut self, tx: &Self::Item, _: InvalidPoolTransactionError) {
		match self.invalid.entry(tx.transaction_id.sender) {
			Entry::Vacant(e) => {
				e.insert(tx.transaction_id);
			}
			Entry::Occupied(mut e) => {
				if e.get().nonce < tx.transaction_id.nonce {
					e.insert(tx.transaction_id);
				}
			}
		}
	}
}

impl<P: Platform> Iterator for PreselectedBestTransactions<P> {
	type Item = Arc<ValidPoolTransaction<P::PooledTransaction>>;

	fn next(&mut self) -> Option<Self::Item> {
		loop {
			let transaction = self.txs.pop()?;

			let Ok(pooled) = P::PooledTransaction::try_from_consensus(transaction)
			else {
				unreachable!("Transaction should be valid at this point");
			};

			let nonce = pooled.nonce();
			let sender_id = self.senders.sender_id_or_create(pooled.sender());

			if let Some(id) = self.invalid.get(&sender_id) {
				if id.nonce <= nonce {
					// transaction or one of its ancestors is marked as invalid, skip it
					continue;
				}
			}

			let wrapper = ValidPoolTransaction {
				transaction: pooled,
				transaction_id: TransactionId::new(sender_id, nonce),
				propagate: false,
				timestamp: std::time::Instant::now(),
				origin: TransactionOrigin::Private,
				authority_ids: None,
			};

			return Some(Arc::new(wrapper));
		}
	}
}
