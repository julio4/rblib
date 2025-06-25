//! Pipeline execution
//!
//! A pipeline is a declarative description of the steps that need to be
//! executed to arrive at a desired payload or signal the inability to do so.
//!
//! The pipeline definition itself does not have any logic for executing
//! steps, enforcing limits or nested pipelines. This is done by the execution
//! engine.
//!
//! Before running the pipeline, the payload will have the platform-specific
//! [`BlockBuilder::apply_pre_execution_changes`] applied to its state.

use {
	crate::{
		pipelines::{
			service::ServiceContext,
			step::{KindTag, WrappedStep},
		},
		traits,
		types,
		BlockContext,
		ControlFlow,
		Pipeline,
		Platform,
		Simulated,
		SimulatedContext,
		SimulatedPayload,
		Static,
		StaticContext,
		StaticPayload,
	},
	core::{
		pin::Pin,
		task::{Context, Poll},
	},
	futures::FutureExt,
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
	tracing::info,
};

/// This type is responsible for executing a single run of a pipeline.
///
/// It's execution is driven by the future poll that it implements. Each call to
/// `poll` will run one step of the pipeline at a time, until the pipeline
/// execution is complete or an error occurs.
pub(super) struct PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	pipeline: Arc<Pipeline>,
	block: Arc<BlockContext<P>>,
	service: Arc<ServiceContext<P, Provider, Pool>>,
	cursor: Cursor<P>,
}

impl<
		P: Platform,
		Provider: traits::ProviderBounds<P>,
		Pool: traits::PoolBounds<P>,
	> PipelineExecutor<P, Provider, Pool>
{
	pub fn run(
		pipeline: Arc<Pipeline>,
		block: Arc<BlockContext<P>>,
		service: Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self {
		info!("pipeline execution started");

		let cursor = match pipeline.first_step_path() {
			Some(path) => {
				let step = pipeline.step_by_path(path.as_slice()).expect(
					"Step path is unreachable. This is a bug in the pipeline executor \
					 implementation.",
				);

				let input = match step.kind() {
					// If the step is static, we can use the static context and payload.
					KindTag::Static => StepInput::Static(StaticContext, StaticPayload),
					// If the step is simulated, we can use the simulated context and
					// payload.
					KindTag::Simulated => {
						StepInput::Simulated(SimulatedContext, SimulatedPayload)
					}
				};

				Cursor::BeforeStep(path, input)
			}
			None => {
				// If the pipeline is empty, we can build an empty payload and complete
				// the pipeline immediately. There is nothing to execute.
				Cursor::<P>::Completed(
					P::construct_payload(
						block.start(),
						service.pool(),
						service.provider(),
					)
					.map_err(ClonablePayloadBuilderError),
				)
			}
		};

		Self {
			pipeline,
			block,
			service,
			cursor,
		}
	}

	pub fn pipeline(&self) -> &Pipeline {
		&self.pipeline
	}

	pub fn block(&self) -> &BlockContext<P> {
		&self.block
	}

	pub fn service(&self) -> &ServiceContext<P, Provider, Pool> {
		&self.service
	}

	pub fn execute(
		&mut self,
	) -> Result<types::BuiltPayload<P>, PayloadBuilderError> {
		todo!()
	}
}

/// private implementation details for the `PipelineExecutor`.
impl<
		P: Platform,
		Provider: traits::ProviderBounds<P>,
		Pool: traits::PoolBounds<P>,
	> PipelineExecutor<P, Provider, Pool>
{
	async fn run_step(step: WrappedStep, input: StepInput) -> StepOutput {
		todo!()
		// match (step.kind(), input) {
		// 	(KindTag::Static, StepInput::Static(ctx, payload)) => {
		// 		let result = step.execute::<Static>(payload, &mut ctx).await;
		// 		StepOutput::Static(result)
		// 	}
		// 	(KindTag::Simulated, StepInput::Simulated(ctx, payload)) => {
		// 		let result = step.execute::<Simulated>(payload, &mut ctx).await;
		// 		StepOutput::Simulated(result)
		// 	}
		// 	(_, _) => {
		// 		unreachable!(
		// 			"Step kind and input type mismatch. This is a bug in the \
		// 			 PipelineExecutor implementation."
		// 		)
		// 	}
		// }
	}

	fn advance_cursor(
		&self,
		current_step_path: &[usize],
		output: StepOutput,
	) -> Cursor<P> {
		todo!("advance_cursor")
	}
}

impl<P, Provider, Pool> Future for PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	type Output = Result<types::BuiltPayload<P>, ClonablePayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let executor = self.get_mut();

		// the pipieline has completed its execution, payload or error is ready.
		if let Cursor::Completed(ref result) = executor.cursor {
			// If the cursor is in the Completed state, we can return the result
			// and resolve the future, no more steps will be executed.
			return Poll::Ready(result.clone());
		}

		if matches!(executor.cursor, Cursor::BeforeStep(_, _)) {
			// If the cursor is in the BeforeStep state, we need to run the next
			// step of the pipeline. Steps are async futures, so we need to store
			// their instance while they are running and being polled.

			let Cursor::BeforeStep(path, input) =
				std::mem::replace(&mut executor.cursor, Cursor::PreparingStep)
			else {
				unreachable!();
			};

			let step = executor.pipeline.step_by_path(path.as_slice()).expect(
				"Step path is unreachable. This is a bug in the pipeline executor \
				 implementation.",
			);

			let running_future = Self::run_step(step.clone(), input);
			executor.cursor = Cursor::InProgress(path, Box::pin(running_future));
			cx.waker().wake_by_ref(); // tell the async runtime to poll again
		}

		if let Cursor::InProgress(ref path, ref mut pinned_future) = executor.cursor
		{
			// If the cursor is in the InProgress state, we need to poll the future
			// to see if it has completed.
			match pinned_future.as_mut().poll_unpin(cx) {
				Poll::Ready(output) => {
					executor.cursor = executor.advance_cursor(path, output);
					cx.waker().wake_by_ref(); // tell the async runtime to poll again
				}
				Poll::Pending => {
					// The future is still running, we can't make any progress.
					return Poll::Pending;
				}
			}
		}

		unreachable!(
			"PipelineExecutor is in an unexpected state. This is a bug in the \
			 PipelineExecutor implementation."
		);
	}
}

/// Keeps track of the current pipeline execution progress.
enum Cursor<P: Platform> {
	/// The pipeline will execute this step on the next iteration.
	/// The `Vec<usize>` contains the indices of the steps that will be executed
	/// in the next iteration. It's a vector because the pipeline can have
	/// nested pipelines, each nesting level will add its own index to the
	/// vector.
	BeforeStep(Vec<usize>, StepInput),

	/// The pipeline finished executing all steps or encountered an error.
	/// This state resolved the executor future with the result of the
	/// pipeline execution.
	Completed(Result<types::BuiltPayload<P>, ClonablePayloadBuilderError>),

	/// a pipeline step execution is in progress.
	///
	/// This state is set when the pipeline executor has began executing a step
	/// but has not yet completed it between two polls. Steps are asynchronous
	/// and may span multiple polls. Until the step future is resolved, we need
	/// to store its instance and poll it.
	///
	/// Here we store the step path that is currently being executed
	/// and the pinned future that is executing the step.
	InProgress(
		Vec<usize>,
		Pin<Box<dyn Future<Output = StepOutput> + Send + Sync + 'static>>,
	),

	/// The pipeline is currently preparing to execute the next step.
	/// We are in this state only for a brief moment inside the `poll` method
	/// and it will never be seen by the `run_step` method.
	PreparingStep,
}

enum StepInput {
	Static(StaticContext, StaticPayload),
	Simulated(SimulatedContext, SimulatedPayload),
}

enum StepOutput {
	Static(ControlFlow<Static>),
	Simulated(ControlFlow<Simulated>),
}

fn build_empty_payload<P, Provider, Pool>(
	executor: &mut PipelineExecutor<P, Provider, Pool>,
) -> Result<types::BuiltPayload<P>, PayloadBuilderError>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	// This function should build an empty payload based on the pipeline and
	// attributes. For now, we return a placeholder.
	info!(
		"Building empty payload for pipeline: {} and attributes: {:?}",
		executor.pipeline(),
		executor.block().attributes()
	);

	// start a new payload, don't add any transactions and build it immediately.
	let checkpoint = executor.block().start();
	P::construct_payload(
		checkpoint,
		executor.service().pool(),
		executor.service().provider(),
	)
}

/// This is a hack to allow cloning of the `PayloadBuilderError`.
/// Because the same result of this future is cached and reuesed
/// in the `PayloadJob`, we need to be able to clone the error so
/// we can use it with `futures::Shared`
pub(super) struct ClonablePayloadBuilderError(PayloadBuilderError);

impl Clone for ClonablePayloadBuilderError {
	fn clone(&self) -> Self {
		match &self.0 {
			PayloadBuilderError::MissingParentHeader(fixed_bytes) => {
				Self(PayloadBuilderError::MissingParentHeader(*fixed_bytes))
			}
			PayloadBuilderError::MissingParentBlock(fixed_bytes) => {
				Self(PayloadBuilderError::MissingParentBlock(*fixed_bytes))
			}
			PayloadBuilderError::ChannelClosed => {
				Self(PayloadBuilderError::ChannelClosed)
			}
			PayloadBuilderError::MissingPayload => {
				Self(PayloadBuilderError::MissingPayload)
			}
			PayloadBuilderError::Internal(reth_error) => {
				Self(PayloadBuilderError::Internal(todo!()))
			}
			PayloadBuilderError::EvmExecutionError(error) => {
				todo!()
			}
			PayloadBuilderError::Other(error) => todo!(),
		}
	}
}

impl From<ClonablePayloadBuilderError> for PayloadBuilderError {
	fn from(error: ClonablePayloadBuilderError) -> Self {
		error.0
	}
}
