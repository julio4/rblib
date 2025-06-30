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
	pipelines_macros::impl_into_pipeline_steps,
	reth::builder::components::PayloadServiceBuilder,
	std::sync::OnceLock,
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
		step: impl Step<Kind = Mode>,
	) -> Self {
		let mut this = self;
		this.prologue = Some(WrappedStep::new(step));
		this
	}

	/// A step that happens as the last step of the block after the whole payload
	/// has been built.
	pub fn with_epilogue<Mode: StepKind>(
		self,
		step: impl Step<Kind = Mode>,
	) -> Self {
		let mut this = self;
		this.epilogue = Some(WrappedStep::new(step));
		this
	}

	/// A step that runs with an input that is the result of the previous step.
	/// The order of steps definition is important, as it determines the flow of
	/// data through the pipeline.
	pub fn with_step<Mode: StepKind>(self, step: impl Step<Kind = Mode>) -> Self {
		let mut this = self;
		this
			.steps
			.push(StepOrPipeline::Step(WrappedStep::new(step)));
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
		this.limits = Some(Box::new(limits));
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

	/// Returns the path to the first step in the pipeline.
	/// Returns `None` if the pipeline is empty.
	pub(crate) fn first_step_path(&self) -> Option<Vec<usize>> {
		if self.is_empty() {
			return None;
		}

		let mut path = vec![0];
		if let Some(first_step) = self.steps.first() {
			path.extend(first_step.first_step());
		}
		Some(path)
	}

	/// Returns a reference to a wrapped step by its index path.
	pub(crate) fn step_by_path(&self, path: &[usize]) -> Option<&WrappedStep<P>> {
		if path.is_empty() {
			return None;
		}

		let index = path[0];
		let item = self.steps.get(index)?;
		item.step_by_path(&path[1..])
	}

	/// Returns a reference to the enclosing pipeline for a step by its index
	/// path.
	pub(crate) fn pipeline_for_step<'a>(
		&'a self,
		path: &[usize],
	) -> Option<(&'a Pipeline<P>, Behavior)> {
		if path.is_empty() {
			return None;
		}

		fn visit<'a, P: Platform>(
			path: &[usize],
			pipeline: &'a Pipeline<P>,
			enclosing_behavior: Behavior,
		) -> Option<(&'a Pipeline<P>, Behavior)> {
			match pipeline.steps.get(path[0])? {
				StepOrPipeline::Step(_) => Some((pipeline, enclosing_behavior)),
				StepOrPipeline::Pipeline(behavior, pipeline) => {
					if path.len() == 1 {
						Some((pipeline, *behavior))
					} else {
						pipeline.pipeline_for_step(&path[1..])
					}
				}
			}
		}

		// top-level pipeline is always `Once` behavior
		visit(path, self, Once)
	}

	/// Executes a closure for each step in the pipeline, including prologue and
	/// epilogue.
	///
	/// This is used by the executor to call maintenance methods at various points
	/// in the node lifecycle, such as `on_spawn`.
	pub(crate) fn for_each_step<F>(&mut self, f: &mut F)
	where
		F: FnMut(&mut WrappedStep<P>),
	{
		if let Some(prologue) = &mut self.prologue {
			f(prologue);
		}

		for step_or_pipeline in &mut self.steps {
			match step_or_pipeline {
				StepOrPipeline::Step(step) => f(step),
				StepOrPipeline::Pipeline(_, pipeline) => {
					pipeline.for_each_step(f);
				}
			}
		}

		if let Some(epilogue) = &mut self.epilogue {
			f(epilogue);
		}
	}
}

pub(crate) enum StepOrPipeline<P: Platform> {
	Step(WrappedStep<P>),
	Pipeline(Behavior, Pipeline<P>),
}

impl<P: Platform> core::fmt::Debug for StepOrPipeline<P> {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			StepOrPipeline::Step(step) => step.fmt(f),
			StepOrPipeline::Pipeline(behavior, pipeline) => f
				.debug_tuple("Pipeline")
				.field(behavior)
				.field(pipeline)
				.finish(),
		}
	}
}

impl<P: Platform> StepOrPipeline<P> {
	/// Returns a reference to a wrapped step by its indecies path.
	/// Each level of the path corresponds to a step in a nested pipeline,
	/// it is expected that level-1 will always be a nested pipeline.
	pub fn step_by_path(&self, path: &[usize]) -> Option<&WrappedStep<P>> {
		if path.is_empty() {
			return match self {
				StepOrPipeline::Step(step) => Some(step),
				StepOrPipeline::Pipeline(_, _) => None,
			};
		}

		match self {
			StepOrPipeline::Step(_) => None,
			StepOrPipeline::Pipeline(_, pipeline) => {
				let index = path[0];
				let item = pipeline.steps.get(index)?;
				item.step_by_path(&path[1..])
			}
		}
	}

	/// Returns the initial path of the first step in this step or pipeline.
	pub fn first_step(&self) -> Vec<usize> {
		match self {
			StepOrPipeline::Step(_) => vec![],
			StepOrPipeline::Pipeline(_, pipeline) => {
				let mut path = vec![0];
				if let Some(first_step) = pipeline.steps.first() {
					path.extend(first_step.first_step());
				}
				path
			}
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

impl<P: Platform, M0: StepKind, S0: Step<Kind = M0>> IntoPipeline<P, ()>
	for (S0,)
{
	fn into_pipeline(self) -> Pipeline<P> {
		Pipeline::default().with_step(self.0)
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
			.field("unique_id", &hex::encode(self.unique_id().to_le_bytes()))
			.field("prologue", &self.prologue.as_ref().map(|p| p.name()))
			.field("epilogue", &self.epilogue.as_ref().map(|e| e.name()))
			.field("steps", &self.steps)
			.field(
				"limits",
				&self.limits.as_ref().map(|l| type_name_of_val(&l)),
			)
			.finish()
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

		assert!(pipeline.step_by_path(&[]).is_none());
		assert_eq!(pipeline.first_step_path(), Some(vec![0]));

		assert!(pipeline.step_by_path(&[0]).is_some());
		assert!(pipeline
			.step_by_path(&[0])
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(pipeline.step_by_path(&[1]).is_some());
		assert!(pipeline
			.step_by_path(&[1])
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(pipeline.step_by_path(&[2]).is_some());
		assert!(pipeline
			.step_by_path(&[2])
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"),);

		assert!(pipeline.step_by_path(&[3]).is_some());
		assert!(pipeline
			.step_by_path(&[3])
			.unwrap()
			.name()
			.ends_with("RevertProtection"));

		assert!(pipeline.step_by_path(&[4]).is_none());
		assert!(pipeline.step_by_path(&[1, 2]).is_none());
	}

	#[test]
	fn step_by_path_nested() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(BuilderEpilogue)
			.with_step(OptimismPrologue)
			.with_pipeline(
				Loop,
				(
					GatherBestTransactions,
					PriorityFeeOrdering,
					TotalProfitOrdering,
				),
			)
			.with_step(RevertProtection);

		assert!(pipeline.step_by_path(&[]).is_none());

		assert_eq!(pipeline.first_step_path(), Some(vec![0]));
		assert!(pipeline.step_by_path(&[0]).is_some());

		assert!(pipeline
			.step_by_path(&[0])
			.unwrap()
			.name()
			.ends_with("OptimismPrologue"));

		assert!(pipeline.step_by_path(&[1]).is_none());
		assert!(pipeline.step_by_path(&[1, 0]).is_some());
		assert!(pipeline.step_by_path(&[1, 0, 0]).is_none());
		assert!(pipeline
			.step_by_path(&[1, 0])
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(pipeline.step_by_path(&[1, 1]).is_some());
		assert!(pipeline
			.step_by_path(&[1, 1])
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(pipeline.step_by_path(&[1, 2]).is_some());
		assert!(pipeline
			.step_by_path(&[1, 2])
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"));
		assert!(pipeline.step_by_path(&[1, 3]).is_none());
		assert!(pipeline.step_by_path(&[2]).is_some());
		assert!(pipeline
			.step_by_path(&[2])
			.unwrap()
			.name()
			.ends_with("RevertProtection"));
		assert!(pipeline.step_by_path(&[3]).is_none());
		assert!(pipeline.step_by_path(&[1, 4]).is_none());
		assert!(pipeline.step_by_path(&[1, 0, 1]).is_none());
	}
}
