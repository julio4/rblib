//! Builder Pipelines API
//!
//! This API is used to construct payload builders.

use {
	crate::{
		pipelines::step::{StepKind, WrappedStep},
		*,
	},
	alloc::{boxed::Box, vec::Vec},
	core::marker::PhantomData,
	pipelines_macros::impl_into_pipeline_steps,
	reth::builder::components::PayloadServiceBuilder,
};

mod exec;
mod limits;
mod service;
mod simulated;
mod r#static;
mod step;

#[cfg(test)]
mod tests;

// public API exports
pub use {
	limits::Limits,
	r#static::{Static, StaticContext, StaticPayload},
	simulated::{Simulated, SimulatedContext, SimulatedPayload},
	step::{ControlFlow, Step},
	Behavior::{Loop, Once},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
	Once,
	Loop,
}

#[derive(Default)]
pub struct Pipeline {
	epilogue: Option<WrappedStep>,
	prologue: Option<WrappedStep>,
	steps: Vec<StepOrPipeline>,
	limits: Option<Box<dyn Limits>>,
}

/// Public API
impl Pipeline {
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
		nested: impl IntoPipeline<T>,
	) -> Self {
		let mut this = self;
		let nested_pipeline = nested.into_pipeline();
		this
			.steps
			.push(StepOrPipeline::Pipeline(behavior, nested_pipeline));
		this
	}

	/// Sets payload building limits for the pipeline.
	pub fn with_limits<L: Limits>(self, limits: L) -> Self {
		let mut this = self;
		this.limits = Some(Box::new(limits));
		this
	}

	/// Converts the pipeline into a payload builder service instance that
	/// can be used when constructing a reth node.
	pub fn into_service<P: Platform, Node, Pool, EvmConfig>(
		self,
	) -> impl PayloadServiceBuilder<Node, Pool, EvmConfig>
	where
		Node: service::traits::NodeBounds<P>,
		Pool: service::traits::PoolBounds<P>,
		EvmConfig: service::traits::EvmConfigBounds<P>,
	{
		service::PipelineServiceBuilder::<P>(self, PhantomData)
	}
}

/// Internal API
impl Pipeline {
	pub(crate) fn prologue(&mut self) -> Option<&mut WrappedStep> {
		self.prologue.as_mut()
	}

	pub(crate) fn epilogue(&mut self) -> Option<&mut WrappedStep> {
		self.epilogue.as_mut()
	}

	pub(crate) fn steps(&mut self) -> &mut [StepOrPipeline] {
		&mut self.steps
	}

	pub(crate) fn limits(&self) -> Option<&dyn Limits> {
		self.limits.as_deref()
	}
}

pub(crate) enum StepOrPipeline {
	Step(WrappedStep),
	Pipeline(Behavior, Pipeline),
}

impl core::fmt::Debug for StepOrPipeline {
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

pub trait IntoPipeline<Marker = ()> {
	fn into_pipeline(self) -> Pipeline;
}

struct Sentinel;
impl<F: FnOnce(Pipeline) -> Pipeline> IntoPipeline<Sentinel> for F {
	fn into_pipeline(self) -> Pipeline {
		self(Pipeline::default())
	}
}

impl IntoPipeline<()> for Pipeline {
	fn into_pipeline(self) -> Pipeline {
		self
	}
}

impl<M0: StepKind, S0: Step<Kind = M0>> IntoPipeline<()> for (S0,) {
	fn into_pipeline(self) -> Pipeline {
		Pipeline::default().with_step(self.0)
	}
}

// Generate implementations for tuples of steps up to 32 elements
impl_into_pipeline_steps!(32);

impl core::fmt::Debug for Pipeline {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("Pipeline")
			.field("prologue", &self.prologue.as_ref().map(|p| p.name()))
			.field("epilogue", &self.epilogue.as_ref().map(|e| e.name()))
			.field("steps", &self.steps)
			.field("limits", &self.limits)
			.finish()
	}
}

mod sealed {
	pub trait Sealed {}
}
