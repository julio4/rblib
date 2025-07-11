use {
	crate::{
		BlockContext,
		Limits,
		Platform,
		pipelines::service::ServiceContext,
		traits,
		types,
	},
	pool::TransactionPool,
	reth::{primitives::SealedHeader, providers::StateProvider},
};

mod pool;

pub struct StepContext<Plat: Platform> {
	block: BlockContext<Plat>,
	pool: TransactionPool<Plat>,
	limits: Limits,
}

impl<P: Platform> StepContext<P> {
	pub fn new<Pool, Provider>(
		block: BlockContext<P>,
		service: &ServiceContext<P, Provider, Pool>,
		limits: Limits,
	) -> Self
	where
		Pool: traits::PoolBounds<P>,
		Provider: traits::ProviderBounds<P>,
	{
		let pool = TransactionPool::new(service.pool().clone());

		Self {
			block,
			pool,
			limits,
		}
	}

	/// Access to the state of the chain at the begining of block that we are
	/// building. This state does not include any changes made by the pipeline
	/// during the payload building process. It does however include changes
	/// applied by platform-specific [`BlockBuilder::apply_pre_execution_changes`]
	/// for this block.
	pub fn provider(&self) -> &dyn StateProvider {
		self.block.base_state()
	}

	/// Parent block header of the block that we are building.
	pub fn parent(&self) -> &SealedHeader<types::Header<P>> {
		self.block.parent()
	}

	/// Access to the block context of the block that we are building.
	pub fn block(&self) -> &BlockContext<P> {
		&self.block
	}

	/// Access to the transaction pool
	pub const fn pool(&self) -> &impl traits::PoolBounds<P> {
		&self.pool
	}

	pub const fn limits(&self) -> &Limits {
		&self.limits
	}
}

#[cfg(any(test, feature = "test-utils"))]
impl<P: Platform> StepContext<P> {}
