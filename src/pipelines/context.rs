use {
	crate::{types, BlockContext, Platform},
	reth::providers::StateProvider,
	reth_transaction_pool::{BestTransactions, PoolTransaction},
	std::sync::Arc,
};

pub struct StepContext<Plat: Platform> {
	block: BlockContext<Plat>,
}

impl<P: Platform> StepContext<P> {
	pub fn new(block: BlockContext<P>) -> Self {
		Self { block }
	}

	/// Access to the state of the chain at the begining of block that we are
	/// building. This state does not include any changes made by the pipeline
	/// during the payload building process. It does however include changes
	/// applied by platform-specific [`BlockBuilder::apply_pre_execution_changes`]
	/// for this block.
	///
	/// Intermediate state changes that are made by the pipeline are only
	/// available in simulated steps through the simulated payload type.
	pub fn provider(&self) -> &dyn StateProvider {
		self.block.base_state()
	}

	pub fn pool_transactions(
		&self,
	) -> Box<
		dyn BestTransactions<
			Item: Arc<impl PoolTransaction<Consensus = types::Transaction<P>>>,
		>,
	> {
		todo!()
	}
}
