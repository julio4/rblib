//! Builder Pipelines API
//!
//! This API is used to construct payload builders.

use {
	crate::pipeline::{
		limits::Limits,
		step::{StepMode, WrappedStep},
	},
	alloc::{boxed::Box, vec::Vec},
	pipeline_macros::impl_into_pipeline_steps,
};
// public API exports
pub use {
	r#static::{Static, StaticContext, StaticPayload},
	simulated::{Simulated, SimulatedContext, SimulatedPayload},
	step::{ControlFlow, Step},
	Behavior::{Loop, Once},
};

mod exec;
mod limits;
mod simulated;
mod r#static;
mod step;

#[cfg(test)]
mod tests;

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
	pub fn with_prologue<Mode: StepMode>(
		self,
		step: impl Step<Mode = Mode>,
	) -> Self {
		let mut this = self;
		this.prologue = Some(WrappedStep::new(step));
		this
	}

	/// A step that happens as the last step of the block after the whole payload
	/// has been built.
	pub fn with_epilogue<Mode: StepMode>(
		self,
		step: impl Step<Mode = Mode>,
	) -> Self {
		let mut this = self;
		this.epilogue = Some(WrappedStep::new(step));
		this
	}

	/// A step that runs with an input that is the result of the previous step.
	/// The order of steps definition is important, as it determines the flow of
	/// data through the pipeline.
	pub fn with_step<Mode: StepMode>(self, step: impl Step<Mode = Mode>) -> Self {
		let mut this = self;
		this
			.steps
			.push(StepOrPipeline::Step(WrappedStep::new(step)));
		this
	}

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

	pub fn with_limits<L: Limits>(self, limits: L) -> Self {
		let mut this = self;
		this.limits = Some(Box::new(limits));
		this
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

impl<M0: StepMode, S0: Step<Mode = M0>> IntoPipeline<()> for (S0,) {
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

#[cfg(test)]
mod test {
	use {super::*, crate::pipeline::tests::steps::*};

	#[test]
	fn pipeline_syntax_only_steps() {
		let pipeline = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_step(GatherBestTransactions)
			.with_step(PriorityFeeOrdering)
			.with_step(TotalProfitOrdering)
			.with_step(RevertProtection);

		println!("{pipeline:#?}");
	}

	#[test]
	fn pipeline_syntax_nested_verbose() {
		let top_level = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue);

		let nested = Pipeline::default()
			.with_step(AppendNewTransactionFromPool)
			.with_step(PriorityFeeOrdering)
			.with_step(TotalProfitOrdering)
			.with_step(RevertProtection);

		let top_level = top_level //
			.with_pipeline(Loop, nested);

		println!("{top_level:#?}");
	}

	#[test]
	fn pipeline_syntax_nested_one_concise_static() {
		let top_level = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_pipeline(Loop, AppendNewTransactionFromPool);

		println!("{top_level:#?}");
	}

	#[test]
	fn pipeline_syntax_nested_one_concise_simulated() {
		let _top_level = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_pipeline(Loop, (RevertProtection,));

		assert!(true, "Pipeline created successfully");
	}

	#[test]
	fn pipeline_syntax_nested_many_concise() {
		let top_level = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_pipeline(
				Loop,
				(
					AppendNewTransactionFromPool,
					PriorityFeeOrdering,
					TotalProfitOrdering,
					RevertProtection,
				),
			);

		println!("{top_level:#?}");
	}

	#[test]
	fn pipeline_syntax_flashblocks_example() {
		struct WebSocketBeginBlock;
		impl Step for WebSocketBeginBlock {
			type Mode = Simulated;

			async fn step(
				&mut self,
				_payload: SimulatedPayload,
				_ctx: &mut SimulatedContext,
			) -> ControlFlow<Simulated> {
				todo!()
			}
		}

		struct WebSocketEndBlock;
		impl Step for WebSocketEndBlock {
			type Mode = Simulated;

			async fn step(
				&mut self,
				_payload: SimulatedPayload,
				_ctx: &mut SimulatedContext,
			) -> ControlFlow<Simulated> {
				todo!()
			}
		}

		struct FlashblockEpilogue;
		impl Step for FlashblockEpilogue {
			type Mode = Simulated;

			async fn step(
				&mut self,
				_payload: SimulatedPayload,
				_ctx: &mut SimulatedContext,
			) -> ControlFlow<Simulated> {
				todo!()
			}
		}

		struct PublishToWebSocket;
		impl Step for PublishToWebSocket {
			type Mode = Simulated;

			async fn step(
				&mut self,
				_payload: SimulatedPayload,
				_ctx: &mut SimulatedContext,
			) -> ControlFlow<Simulated> {
				todo!()
			}
		}

		#[derive(Debug)]
		struct FlashblockLimits;
		impl Limits for FlashblockLimits {}

		let fb = Pipeline::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_step(WebSocketBeginBlock)
			.with_pipeline(Loop, |nested: Pipeline| {
				nested
					.with_limits(FlashblockLimits)
					.with_epilogue(FlashblockEpilogue)
					.with_pipeline(
						Loop,
						(
							AppendNewTransactionFromPool,
							PriorityFeeOrdering,
							TotalProfitOrdering,
							RevertProtection,
						),
					)
					.with_step(PublishToWebSocket)
			})
			.with_step(WebSocketEndBlock);

		println!("{fb:#?}");
	}
}
