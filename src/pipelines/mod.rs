//! Builder Pipelines API
//!
//! This API is used to construct payload builders.

use {
	crate::{
		pipelines::step::{StepKind, WrappedStep},
		*,
	},
	alloy::hex,
	core::{any::type_name_of_val, fmt::Display},
	derive_more::{Deref, DerefMut, From, Into},
	pipelines_macros::impl_into_pipeline_steps,
	reth::builder::components::PayloadServiceBuilder,
	smallvec::{smallvec, SmallVec},
	std::sync::{Arc, OnceLock},
};

mod context;
mod exec;
mod job;
mod limits;
mod service;
mod simulated;
mod r#static;
mod step;
pub mod steps;

#[cfg(test)]
mod tests;

// public API exports
pub use {
	context::StepContext,
	limits::{Limits, LimitsFactory},
	r#static::{Static, StaticPayload},
	simulated::{Simulated, SimulatedPayload},
	step::{ControlFlow, Step},
	Behavior::{Loop, Once},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
	Once,
	Loop,
}

pub struct Pipeline<P: Platform> {
	epilogue: Option<WrappedStep<P>>,
	prologue: Option<WrappedStep<P>>,
	steps: Vec<StepOrPipeline<P>>,
	limits: Option<Box<dyn LimitsFactory<P>>>,
	unique_id: OnceLock<usize>,
}

impl<P: Platform> Default for Pipeline<P> {
	fn default() -> Self {
		Self {
			epilogue: None,
			prologue: None,
			steps: Vec::new(),
			limits: None,
			unique_id: OnceLock::new(),
		}
	}
}

/// Public API
impl<P: Platform> Pipeline<P> {
	/// A step that happens before any transaction is added to the block, executes
	/// as the first step in the pipeline.
	pub fn with_prologue<Mode: StepKind>(
		self,
		step: impl Step<P, Kind = Mode>,
	) -> Self {
		let mut this = self;
		this.prologue = Some(WrappedStep::new(step));
		this
	}

	/// A step that happens as the last step of the block after the whole payload
	/// has been built.
	pub fn with_epilogue<Mode: StepKind>(
		self,
		step: impl Step<P, Kind = Mode>,
	) -> Self {
		let mut this = self;
		this.epilogue = Some(WrappedStep::new(step));
		this
	}

	/// A step that runs with an input that is the result of the previous step.
	/// The order of steps definition is important, as it determines the flow of
	/// data through the pipeline.
	pub fn with_step<Mode: StepKind>(
		self,
		step: impl Step<P, Kind = Mode>,
	) -> Self {
		let mut this = self;
		this
			.steps
			.push(StepOrPipeline::Step(Arc::new(WrappedStep::new(step))));
		this
	}

	/// Adds a nested pipeline to the current pipeline.
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
	pub(crate) fn prologue(&self) -> Option<&WrappedStep<P>> {
		self.prologue.as_ref()
	}

	pub(crate) fn epilogue(&self) -> Option<&WrappedStep<P>> {
		self.epilogue.as_ref()
	}

	pub(crate) fn steps(&self) -> &[StepOrPipeline<P>] {
		&self.steps
	}

	pub(crate) fn limits(&self) -> Option<&dyn LimitsFactory<P>> {
		self.limits.as_deref()
	}

	/// A unique identifier of the pipieline instance.
	///
	/// This is used mostly for debug printing and logging purposes.
	/// Do not rely on this value for any logic, as it is not guaranteed to be
	/// unique across different runs of the program.
	pub(crate) fn unique_id(&self) -> usize {
		*self.unique_id.get_or_init(|| self as *const Self as usize)
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

impl<P: Platform, M0: StepKind, S0: Step<P, Kind = M0>> IntoPipeline<P, ()>
	for (S0,)
{
	fn into_pipeline(self) -> Pipeline<P> {
		Pipeline::default().with_step(self.0)
	}
}

impl<P: Platform, M0: StepKind, S0: Step<P, Kind = M0>> IntoPipeline<P, u8>
	for S0
{
	fn into_pipeline(self) -> Pipeline<P> {
		Pipeline::default().with_step(self)
	}
}

// Generate implementations for tuples of steps up to 32 elements
impl_into_pipeline_steps!(32);

impl<P: Platform> Display for Pipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"Pipeline(0x{}, {} steps)",
			hex::encode(self.unique_id().to_le_bytes()),
			self.steps.len()
		)
	}
}

impl<P: Platform> core::fmt::Debug for Pipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Pipeline")
			.field(
				"unique_id",
				&hex::encode(self.unique_id().to_le_bytes()) as &dyn core::fmt::Debug,
			)
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

#[derive(Deref, DerefMut, Default, PartialEq, Eq, Clone, Debug, From, Into)]
pub(crate) struct StepPath(SmallVec<[usize; 8]>);

impl StepPath {
	pub fn new(path: impl Into<SmallVec<[usize; 8]>>) -> Self {
		let path: SmallVec<[usize; 8]> = path.into();
		assert!(!path.is_empty(), "StepPath cannot be empty");
		Self(path)
	}

	pub fn zero() -> Self {
		Self(smallvec![0])
	}

	/// Returns the path to the first step in the pipeline.
	///
	/// This function will traverse the pipeline and return the innermost
	/// step that is the first in the execution order.
	///
	/// Returns `None` if the pipeline is empty or is composed of only empty
	/// pipelines.
	pub fn first_step<P: Platform>(pipeline: &Pipeline<P>) -> Option<Self> {
		if pipeline.is_empty() {
			return None;
		}

		match pipeline.steps.first()? {
			StepOrPipeline::Step(_) => Some(StepPath::zero()),
			StepOrPipeline::Pipeline(_, pipeline) => {
				Some(StepPath::zero().append(StepPath::first_step(pipeline)?))
			}
		}
	}

	pub fn root(&self) -> Option<usize> {
		self.0.first().copied()
	}

	pub fn pop_front(self) -> Option<StepPath> {
		if self.0.len() < 2 {
			// If there's only one element, popping it would result in an empty path
			// which is not allowed.
			None
		} else {
			Some(StepPath(self.0[1..].into()))
		}
	}

	pub fn pop_back(self) -> Option<StepPath> {
		if self.0.len() < 2 {
			None
		} else {
			Some(StepPath(self.0[..self.0.len() - 1].into()))
		}
	}

	pub fn append(self, other: Self) -> Self {
		let mut new_path = self.0;
		new_path.extend(other.0);
		Self(new_path)
	}

	pub fn next_step(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last += 1;
		}
		Self(new_path)
	}

	pub fn restart_loop(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last = 0;
		}
		Self(new_path)
	}

	pub fn locate<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<&'a StepOrPipeline<P>> {
		if pipeline.is_empty() {
			return None;
		}

		let mut path = self.clone();
		let mut item = pipeline.steps.get(path.root()?)?;

		while let Some(descendant) = path.pop_front() {
			item = match item {
				StepOrPipeline::Step(_) => return None,
				StepOrPipeline::Pipeline(_, pipeline) => {
					pipeline.steps.get(descendant.root()?)?
				}
			};
			path = descendant;
		}

		Some(item)
	}

	pub fn find_step<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<&'a Arc<WrappedStep<P>>> {
		self.locate(pipeline).and_then(|item| match item {
			StepOrPipeline::Step(step) => Some(step),
			StepOrPipeline::Pipeline(_, _) => None,
		})
	}

	pub fn find_pipeline<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<(&'a Pipeline<P>, Behavior)> {
		self.locate(pipeline).and_then(|item| match item {
			StepOrPipeline::Step(_) => None,
			StepOrPipeline::Pipeline(behavior, pipeline) => {
				Some((pipeline, *behavior))
			}
		})
	}

	pub fn is_step<P: Platform>(&self, pipeline: &Pipeline<P>) -> bool {
		self.find_step(pipeline).is_some()
	}

	pub fn is_pipeline<P: Platform>(&self, pipeline: &Pipeline<P>) -> bool {
		self.find_pipeline(pipeline).is_some()
	}
}

impl<const N: usize> From<[usize; N]> for StepPath {
	fn from(array: [usize; N]) -> Self {
		assert!(!array.is_empty(), "StepPath cannot be empty");
		Self(array.into_iter().collect())
	}
}

mod sealed {
	pub trait Sealed {}
}

#[cfg(test)]
mod test {
	use super::{steps::*, *};

	#[test]
	fn step_by_path_flat() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(BuilderEpilogue)
			.with_step(GatherBestTransactions)
			.with_step(PriorityFeeOrdering)
			.with_step(TotalProfitOrdering)
			.with_step(RevertProtection);

		assert!(StepPath::from([90]).find_step(&pipeline).is_none());
		assert_eq!(StepPath::first_step(&pipeline), Some(StepPath::zero()));

		assert!(StepPath::from([0]).find_step(&pipeline).is_some());
		assert!(StepPath::from([0])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(StepPath::from([1]).find_step(&pipeline).is_some());
		assert!(StepPath::from([1])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(StepPath::from([2]).find_step(&pipeline).is_some());
		assert!(StepPath::from([2])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"));

		assert!(StepPath::from([3]).find_step(&pipeline).is_some());
		assert!(StepPath::from([3])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("RevertProtection"));

		assert!(StepPath::from([4]).find_step(&pipeline).is_none());
		assert!(StepPath::from([1, 2]).find_step(&pipeline).is_none());
	}

	#[test]
	fn step_by_path_nested() {
		pub struct TestStep;
		impl<P: Platform> Step<P> for TestStep {
			type Kind = Simulated;

			async fn step(
				self: Arc<Self>,
				_payload: SimulatedPayload<P>,
				_ctx: StepContext<P>,
			) -> ControlFlow<P, Simulated> {
				todo!()
			}
		}

		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(BuilderEpilogue)
			.with_step(TestStep)
			.with_pipeline(
				Loop,
				(
					GatherBestTransactions,
					PriorityFeeOrdering,
					TotalProfitOrdering,
				),
			)
			.with_step(RevertProtection);

		assert!(StepPath::from([99]).find_step(&pipeline).is_none());
		assert_eq!(StepPath::first_step(&pipeline), Some(StepPath::zero()));
		assert!(StepPath::from([0]).find_step(&pipeline).is_some());

		assert!(StepPath::from([0])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TestStep"));

		assert!(StepPath::from([1]).find_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0]).find_step(&pipeline).is_some());
		assert!(StepPath::from([1, 0, 0]).find_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(StepPath::from([1, 1]).find_step(&pipeline).is_some());
		assert!(StepPath::from([1, 1])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(StepPath::from([1, 2]).find_step(&pipeline).is_some());
		assert!(StepPath::from([1, 2])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"));
		assert!(StepPath::from([1, 3]).find_step(&pipeline).is_none());
		assert!(StepPath::from([2]).find_step(&pipeline).is_some());
		assert!(StepPath::from([2])
			.find_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("RevertProtection"));
		assert!(StepPath::from([3]).find_step(&pipeline).is_none());
		assert!(StepPath::from([1, 4]).find_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0, 1]).find_step(&pipeline).is_none());
	}
}
