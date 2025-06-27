use {
	crate::{pipelines::service::ServiceContext, traits, BlockContext, Platform},
	pool::TransactionPool,
	reth::providers::StateProvider,
	std::sync::Arc,
};

mod pool;

pub struct StepContext<Plat: Platform> {
	block: BlockContext<Plat>,
	pool: TransactionPool<Plat>,
}

impl<P: Platform> StepContext<P> {
	pub fn new<Pool, Provider>(
		block: BlockContext<P>,
		service: Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self
	where
		Pool: traits::PoolBounds<P>,
		Provider: traits::ProviderBounds<P>,
	{
		let pool = TransactionPool::new(service.pool().clone());

		Self { block, pool }
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

	/// Access to the transaction pool
	pub fn pool(&self) -> &impl traits::PoolBounds<P> {
		&self.pool
	}
}
