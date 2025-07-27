//! Builder Pipelines API
//!
//! This API is used to construct payload builders workflows.

use {
	crate::{prelude::*, reth::builder::components::PayloadServiceBuilder},
	core::{any::type_name_of_val, fmt::Display},
	pipelines_macros::impl_into_pipeline_steps,
	std::sync::Arc,
	step::WrappedStep,
};

mod context;
mod exec;
mod job;
mod service;
mod step;

#[cfg(test)]
mod tests;

// public API exports
pub use {
	Behavior::{Loop, Once},
	context::StepContext,
	step::{ControlFlow, PayloadBuilderError, Step},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
	Once,
	Loop,
}

pub struct Pipeline<P: Platform> {
	epilogue: Option<Arc<WrappedStep<P>>>,
	prologue: Option<Arc<WrappedStep<P>>>,
	steps: Vec<StepOrPipeline<P>>,
	limits: Option<Box<dyn LimitsFactory<P>>>,
	name: Option<String>,
}

impl<P: Platform> Default for Pipeline<P> {
	/// Creates a new empty unnamed pipeline.
	fn default() -> Self {
		Self {
			epilogue: None,
			prologue: None,
			steps: Vec::new(),
			limits: None,
			name: None,
		}
	}
}

/// Public API
impl<P: Platform> Pipeline<P> {
	/// Creates a new empty pipeline with a name.
	pub fn named(name: impl Into<String>) -> Self {
		Self {
			name: Some(name.into()),
			..Default::default()
		}
	}

	/// A step that happens before any transaction is added to the block, executes
	/// as the first step in the pipeline.
	#[must_use]
	pub fn with_prologue(self, step: impl Step<P>) -> Self {
		let mut this = self;
		this.prologue = Some(Arc::new(WrappedStep::new(step)));
		this
	}

	/// A step that happens as the last step of the block after the whole payload
	/// has been built.
	#[must_use]
	pub fn with_epilogue(self, step: impl Step<P>) -> Self {
		let mut this = self;
		this.epilogue = Some(Arc::new(WrappedStep::new(step)));
		this
	}

	/// A step that runs with an input that is the result of the previous step.
	/// The order of steps definition is important, as it determines the flow of
	/// data through the pipeline.
	#[must_use]
	pub fn with_step(self, step: impl Step<P>) -> Self {
		let mut this = self;
		this
			.steps
			.push(StepOrPipeline::Step(Arc::new(WrappedStep::new(step))));
		this
	}

	/// Adds a nested pipeline to the current pipeline.
	#[must_use]
	pub fn with_pipeline<T>(
		self,
		behavior: Behavior,
		nested: impl IntoPipeline<P, T>,
	) -> Self {
		let mut this = self;
		let nested_pipeline = nested.into_pipeline();
		this
			.steps
			.push(StepOrPipeline::Pipeline(behavior, nested_pipeline));
		this
	}

	/// Sets payload building limits for the pipeline.
	#[must_use]
	pub fn with_limits<L: LimitsFactory<P>>(self, limits: L) -> Self {
		let mut this = self;
		this.limits = Some(Box::new(limits) as Box<dyn LimitsFactory<P>>);
		this
	}

	/// Converts the pipeline into a payload builder service instance that
	/// can be used when constructing a reth node.
	pub fn into_service<Node, Pool, EvmConfig>(
		self,
	) -> impl PayloadServiceBuilder<Node, Pool, EvmConfig>
	where
		Node: traits::NodeBounds<P>,
		Pool: traits::PoolBounds<P>,
		EvmConfig: traits::EvmConfigBounds<P>,
	{
		service::PipelineServiceBuilder::new(self)
	}

	/// Returns true if the pipieline has no steps, prologue or epilogue.
	pub fn is_empty(&self) -> bool {
		self.prologue.is_none() && self.epilogue.is_none() && self.steps.is_empty()
	}
}

/// Internal API
impl<P: Platform> Pipeline<P> {
	pub(crate) fn prologue(&self) -> Option<&Arc<WrappedStep<P>>> {
		self.prologue.as_ref()
	}

	pub(crate) fn epilogue(&self) -> Option<&Arc<WrappedStep<P>>> {
		self.epilogue.as_ref()
	}

	pub(crate) fn steps(&self) -> &[StepOrPipeline<P>] {
		&self.steps
	}

	pub(crate) fn limits(&self) -> Option<&dyn LimitsFactory<P>> {
		self.limits.as_deref()
	}

	/// An optional name of the pipeline.
	///
	/// This is used mostly for debug printing and logging purposes.
	pub fn name(&self) -> Option<&str> {
		self.name.as_deref()
	}

	/// Executes the provided async function for each step in the pipeline once.
	///
	/// This is used for performing initialization or cleanup tasks for each step.
	pub(crate) async fn for_each_step<F, R, E>(&self, f: &F) -> Result<(), E>
	where
		F: Fn(Arc<WrappedStep<P>>) -> R,
		R: Future<Output = Result<(), E>> + Send,
	{
		if let Some(prologue) = &self.prologue {
			f(Arc::clone(prologue)).await?;
		}

		for step in &self.steps {
			match step {
				StepOrPipeline::Step(wrapped_step) => {
					f(Arc::clone(wrapped_step)).await?;
				}
				StepOrPipeline::Pipeline(_, pipeline) => {
					Box::pin(pipeline.for_each_step(f)).await?;
				}
			}
		}

		if let Some(epilogue) = &self.epilogue {
			f(Arc::clone(epilogue)).await?;
		}

		Ok(())
	}
}

pub(crate) enum StepOrPipeline<P: Platform> {
	Step(Arc<WrappedStep<P>>),
	Pipeline(Behavior, Pipeline<P>),
}

impl<P: Platform> core::fmt::Debug for StepOrPipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			StepOrPipeline::Step(step) => step.fmt(f),
			StepOrPipeline::Pipeline(behavior, pipeline) => f
				.debug_tuple("Pipeline")
				.field(behavior as &dyn core::fmt::Debug)
				.field(pipeline as &dyn core::fmt::Debug)
				.finish(),
		}
	}
}

/// This trait is used to enable various syntatic sugar for defining nested
/// pipelines
pub trait IntoPipeline<P: Platform, Marker = ()> {
	fn into_pipeline(self) -> Pipeline<P>;
}

struct Sentinel;
impl<P: Platform, F: FnOnce(Pipeline<P>) -> Pipeline<P>>
	IntoPipeline<P, Sentinel> for F
{
	fn into_pipeline(self) -> Pipeline<P> {
		self(Pipeline::<P>::default())
	}
}

impl<P: Platform> IntoPipeline<P, ()> for Pipeline<P> {
	fn into_pipeline(self) -> Pipeline<P> {
		self
	}
}

impl<P: Platform, S0: Step<P>> IntoPipeline<P, ()> for (S0,) {
	fn into_pipeline(self) -> Pipeline<P> {
		Pipeline::default().with_step(self.0)
	}
}

impl<P: Platform, S0: Step<P>> IntoPipeline<P, u8> for S0 {
	fn into_pipeline(self) -> Pipeline<P> {
		Pipeline::default().with_step(self)
	}
}

// Generate implementations for tuples of steps up to 32 elements
impl_into_pipeline_steps!(32);

/// Helper trait that supports the concise pipeline definition syntax.
pub trait PipelineBuilderExt<P: Platform> {
	fn with_prologue(self, step: impl Step<P>) -> Pipeline<P>;
	fn with_epilogue(self, step: impl Step<P>) -> Pipeline<P>;
	fn with_limits<L: LimitsFactory<P>>(self, limits: L) -> Pipeline<P>;
	fn with_name(self, name: impl Into<String>) -> Pipeline<P>;
}

impl<P: Platform, T: IntoPipeline<P, ()>> PipelineBuilderExt<P> for T {
	fn with_prologue(self, step: impl Step<P>) -> Pipeline<P> {
		self.into_pipeline().with_prologue(step)
	}

	fn with_epilogue(self, step: impl Step<P>) -> Pipeline<P> {
		self.into_pipeline().with_epilogue(step)
	}

	fn with_limits<L: LimitsFactory<P>>(self, limits: L) -> Pipeline<P> {
		self.into_pipeline().with_limits(limits)
	}

	fn with_name(self, name: impl Into<String>) -> Pipeline<P> {
		let mut pipeline = self.into_pipeline();
		pipeline.name = Some(name.into());
		pipeline
	}
}

impl<P: Platform> Display for Pipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"Pipeline({}{} steps)",
			match self.name() {
				Some(name) => format!("name={name}, "),
				None => String::new(),
			},
			self.steps.len()
		)
	}
}

impl<P: Platform> core::fmt::Debug for Pipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Pipeline")
			.field("name", &self.name() as &dyn core::fmt::Debug)
			.field(
				"prologue",
				&self.prologue.as_ref().map(|p| p.name()) as &dyn core::fmt::Debug,
			)
			.field(
				"epilogue",
				&self.epilogue.as_ref().map(|e| e.name()) as &dyn core::fmt::Debug,
			)
			.field("steps", &self.steps as &dyn core::fmt::Debug)
			.field(
				"limits",
				&self.limits.as_ref().map(|l| type_name_of_val(&l))
					as &dyn core::fmt::Debug,
			)
			.finish()
	}
}

pub mod traits {
	use crate::{
		prelude::*,
		reth::{
			api::FullNodeTypes,
			evm::ConfigureEvm,
			providers::{ChainSpecProvider, HeaderProvider, StateProviderFactory},
			transaction_pool::{PoolTransaction, TransactionPool},
		},
	};

	pub trait NodeBounds<P: Platform>:
		FullNodeTypes<Types = P::NodeTypes>
	{
	}

	impl<T, P: Platform> NodeBounds<P> for T where
		T: FullNodeTypes<Types = P::NodeTypes>
	{
	}

	pub trait ProviderBounds<P: Platform>:
		StateProviderFactory
		+ ChainSpecProvider<ChainSpec = types::ChainSpec<P>>
		+ HeaderProvider<Header = types::Header<P>>
		+ Clone
		+ Send
		+ Sync
		+ 'static
	{
	}

	impl<T, P: Platform> ProviderBounds<P> for T where
		T: StateProviderFactory
			+ ChainSpecProvider<ChainSpec = types::ChainSpec<P>>
			+ HeaderProvider<Header = types::Header<P>>
			+ Clone
			+ Send
			+ Sync
			+ 'static
	{
	}

	pub trait PoolBounds<P: Platform>:
		TransactionPool<Transaction = P::PooledTransaction> + Unpin + 'static
	{
	}

	impl<T, P: Platform> PoolBounds<P> for T where
		T: TransactionPool<Transaction = P::PooledTransaction> + Unpin + 'static
	{
	}

	pub trait PooledTransactionBounds<P: Platform>:
		PoolTransaction<Consensus = types::Transaction<P>> + Send + Sync + 'static
	{
	}

	pub trait EvmConfigBounds<P: Platform>:
		ConfigureEvm<Primitives = types::Primitives<P>> + Send + Sync
	{
	}

	impl<T, P: Platform> EvmConfigBounds<P> for T where
		T: ConfigureEvm<Primitives = types::Primitives<P>> + Send + Sync
	{
	}

	pub trait PlatformExecBounds<P: Platform>:
		Platform<NodeTypes = types::NodeTypes<P>, EvmConfig = types::EvmConfig<P>>
	{
	}

	impl<T, P: Platform> PlatformExecBounds<T> for P where
		T: Platform<NodeTypes = types::NodeTypes<P>, EvmConfig = types::EvmConfig<P>>
	{
	}
}

// internal utilities
#[cfg(any(test, feature = "test-utils"))]
pub(crate) use exec::clone_payload_error;
