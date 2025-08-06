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
		pipelines::{exec::navi::StepNavigator, service::ServiceContext},
		prelude::*,
		reth::payload::builder::PayloadBuilderError,
	},
	core::{
		fmt::{Debug, Display},
		pin::Pin,
		task::{Context, Poll},
	},
	futures::FutureExt,
	navi::StepPath,
	std::sync::Arc,
	tracing::{debug, trace},
};

pub(super) mod navi;

type PipelineOutput<P: Platform> =
	Result<types::BuiltPayload<P>, ClonablePayloadBuilderError>;

/// This type is responsible for executing a single run of a pipeline.
///
/// It's execution is driven by the future poll that it implements. Each call to
/// `poll` will run one step of the pipeline at a time, or parts of a step if
/// the step is async and needs many polls before it completes. The executor
/// future will resolve when the whole pipeline has been executed, or when an
/// error occurs.
pub(super) struct PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	cursor: Cursor<P>,
	context: ExecContext<P, Provider, Pool>,
}

impl<
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
> PipelineExecutor<P, Provider, Pool>
{
	/// Begins the execution of a pipeline for a new block.
	pub fn run(
		pipeline: Arc<Pipeline<P>>,
		block: BlockContext<P>,
		service: Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self {
		// Initially set the execution cursor to initializing state, that will call
		// all `before_job` methods of the steps in the pipeline.

		Self {
			cursor: Cursor::<P>::Initializing({
				let block = block.clone();
				let pipeline = Arc::clone(&pipeline);
				let service = Arc::clone(&service);
				async move {
					pipeline
						.for_each_step(&|step_navi: StepNavigator<P>| {
							let context = StepContext::new(&block, &service, &step_navi);
							async move { step_navi.step().before_job(context).await }
						})
						.await
				}
				.boxed()
			}),
			context: ExecContext {
				pipeline,
				block,
				service,
			},
		}
	}
}

/// private implementation details for the `PipelineExecutor`.
impl<
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
> PipelineExecutor<P, Provider, Pool>
{
	fn create_step_context(&self, step: &StepPath) -> StepContext<P> {
		let pipeline = Arc::clone(&self.context.pipeline);
		let step = step.navigator(&pipeline).expect(
			"Invalid step path. This is a bug in the pipeline executor \
			 implementation.",
		);

		StepContext::new(&self.context.block, &self.context.service, &step)
	}

	/// This method creates a future that encapsulates the execution an an async
	/// step.
	///
	/// It will prepare the step context and all the information a pipeline step
	/// needs to execute, then create a future object that will be stored in the
	/// cursor and polled whenever the executor is polled.
	fn execute_step(
		&self,
		path: &StepPath,
		input: Checkpoint<P>,
	) -> Pin<Box<dyn Future<Output = ControlFlow<P>> + Send>> {
		let context = self.create_step_context(path);
		let step = Arc::clone(
			path
				.navigator(&self.context.pipeline)
				.expect(
					"Step path is unreachable. This is a bug in the pipeline executor \
					 implementation.",
				)
				.step(),
		);

		async move { step.execute(input, context).await }.boxed()
	}

	/// This method handles the control flow of the pipeline execution.
	///
	/// Once a step is executed it determines the next step to execute based on
	/// the output of the step, the current cursor state and the pipeline
	/// structure.
	fn advance_cursor(
		&self,
		path: &StepPath,
		output: ControlFlow<P>,
	) -> Cursor<P> {
		// we need this type to determine the next step to execute
		// based on the current step output.
		let navigator = path.navigator(&self.context.pipeline).expect(
			"Step path is unreachable. This is a bug in the pipeline executor \
			 implementation.",
		);

		// identify the next step to execute based on the output of the previously
		// executed step.
		let (step, input) = match output {
			// If the step output is a failure, we terminate the execution of the
			// whole pipeline and return the error as the final output on next future
			// poll.
			ControlFlow::Fail(payload_builder_error) => {
				return Cursor::Finalizing(
					self
						.finalize(Err(ClonablePayloadBuilderError(payload_builder_error))),
				);
			}

			// not a failure, chose the next step based on the control flow output
			ControlFlow::Break(checkpoint) => (navigator.next_break(), checkpoint),
			ControlFlow::Ok(checkpoint) => (navigator.next_ok(), checkpoint),
		};

		let Some(step) = step else {
			// If there is no next step, we are done with the pipeline execution.
			// We can finalize the pipeline and return the output as the final
			// result of the pipeline run.
			return Cursor::Finalizing(
				self.finalize(
					P::build_payload(input, self.context.service.provider())
						.map_err(ClonablePayloadBuilderError),
				),
			);
		};

		// there is a next step to be executed, create a cursor that will
		// start running the next step with the output of the current step
		// as input on next executor future poll
		Cursor::BeforeStep(step.into(), input)
	}

	/// After pipeline steps are initialized, this method will identify the first
	/// step to execute in the pipeline and prepare the cursor to run it.
	fn first_step(&self) -> Cursor<P> {
		let Some(navigator) = StepNavigator::entrypoint(&self.context.pipeline)
		else {
			debug!(
				"empty pipeline, building empty payload for attributes: {:?}",
				self.context.block.attributes()
			);

			return Cursor::<P>::Finalizing(
				self.finalize(
					P::build_payload(
						self.context.block.start(),
						self.context.service.provider(),
					)
					.map_err(ClonablePayloadBuilderError),
				),
			);
		};

		Cursor::BeforeStep(navigator.into(), self.context.block.start())
	}

	/// This method will walk through the pipeline steps and invoke the
	/// `after_job` method of each step in the pipeline with the final output.
	fn finalize(
		&self,
		output: PipelineOutput<P>,
	) -> Pin<Box<dyn Future<Output = PipelineOutput<P>> + Send>> {
		let output = Arc::new(output.map_err(Into::into));
		let pipeline = Arc::clone(&self.context.pipeline);
		let block = self.context.block.clone();
		let pipeline = Arc::clone(&pipeline);
		let service = Arc::clone(&self.context.service);

		async move {
			// invoke the `after_job` method of each step in the pipeline
			// if any of them failes we fail the pipeline execution, othwerwise
			// we return the output of the pipeline.
			pipeline
				.for_each_step(&|step_navi: StepNavigator<P>| {
					let output = Arc::clone(&output);
					let block = block.clone();
					let service = Arc::clone(&service);
					async move {
						let ctx = StepContext::new(&block, &service, &step_navi);
						step_navi.step().after_job(ctx, output).await.map_err(|e| {
							ClonablePayloadBuilderError(PayloadBuilderError::other(
								WrappedErrorMessage(e.to_string()),
							))
						})
					}
				})
				.await?;

			Arc::into_inner(output)
				.expect("unexpected strong reference count")
				.map_err(ClonablePayloadBuilderError)
		}
		.boxed()
	}
}

impl<P, Provider, Pool> Future for PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	type Output = PipelineOutput<P>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let executor = self.get_mut();

		// The executor has not ran any steps yet, it is invoking the `before_job`
		// method of each step in the pipeline.
		if let Cursor::Initializing(ref mut future) = executor.cursor {
			if let Poll::Ready(output) = future.as_mut().poll_unpin(cx) {
				match output {
					Ok(()) => {
						trace!("{} initialized successfully", executor.context.pipeline);
						executor.cursor = executor.first_step();
					}
					Err(error) => {
						trace!(
							"{} initialization failed with error: {error:?}",
							executor.context.pipeline
						);
						// If the initialization failed, we immediately finalize the
						// pipeline with the error that occurred during initialization
						// and not attempt to run any steps.
						executor.cursor = Cursor::Finalizing(
							executor.finalize(Err(ClonablePayloadBuilderError(error))),
						);
					}
				}
				trace!("{} initializing completed", executor.context.pipeline);
			}

			// tell the async runtime to poll again because we are still initializing
			cx.waker().wake_by_ref();
		}

		// the pipeline has completed executing all steps or encountered and error.
		// Now we are running the `after_job` of each step in the pipeline.
		if let Cursor::Finalizing(ref mut future) = executor.cursor {
			if let Poll::Ready(output) = future.as_mut().poll_unpin(cx) {
				trace!(
					"{} completed with output: {output:#?}",
					executor.context.pipeline
				);

				// all execution of this pipeline has completed, This resolves the
				// executor future with the final output of the pipeline.
				return Poll::Ready(output);
			}

			// tell the async runtime to poll again because we are still finalizing
			cx.waker().wake_by_ref();
		}

		if matches!(executor.cursor, Cursor::BeforeStep(_, _)) {
			// If the cursor is in the BeforeStep state, we need to run the next
			// step of the pipeline. Steps are async futures, so we need to store
			// their instance while they are running and being polled.

			let Cursor::BeforeStep(path, input) =
				std::mem::replace(&mut executor.cursor, Cursor::PreparingStep)
			else {
				unreachable!("bug in PipelineExecutor state machine");
			};

			trace!("{} will execute step {path:?}", executor.context.pipeline,);

			let running_future = executor.execute_step(&path, input);
			executor.cursor = Cursor::StepInProgress(path, running_future);
			cx.waker().wake_by_ref(); // tell the async runtime to poll again
		}

		if let Cursor::StepInProgress(ref path, ref mut future) = executor.cursor {
			// If the cursor is in the StepInProgress state, we to poll the
			// future instance of that step to see if it has completed.
			if let Poll::Ready(output) = future.as_mut().poll_unpin(cx) {
				trace!(
					"{} step {path:?} completed with output: {output:#?}",
					executor.context.pipeline
				);

				// step has completed, we can advance the cursor
				executor.cursor = executor.advance_cursor(path, output);
			}

			cx.waker().wake_by_ref(); // tell the async runtime to poll again
		}

		Poll::Pending
	}
}

struct ExecContext<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	// The pipeline that is being executed.
	pipeline: Arc<Pipeline<P>>,
	// The block context that is being built.
	block: BlockContext<P>,
	// The service context that provides access to the transaction pool and state
	// provider.
	service: Arc<ServiceContext<P, Provider, Pool>>,
}

/// Keeps track of the current pipeline execution progress.
enum Cursor<P: Platform> {
	/// The pipeline will execute this step on the next iteration.
	BeforeStep(StepPath, Checkpoint<P>),

	/// a pipeline step execution is in progress.
	///
	/// This state is set when the pipeline executor has began executing a step
	/// but has not yet completed it between two polls. Steps are asynchronous
	/// and may span multiple polls. Until the step future is resolved, we need
	/// to store its instance and poll it.
	///
	/// Here we store the step path that is currently being executed
	/// and the pinned future that is executing the step.
	StepInProgress(
		StepPath,
		Pin<Box<dyn Future<Output = ControlFlow<P>> + Send>>,
	),

	/// The pipeline is currently initializing all steps for a new payload job.
	///
	/// This happens once before any step is executed and it calls the
	/// `before_job` method of each step in the pipeline.
	Initializing(
		Pin<Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>>,
	),

	/// This state occurs after the `Completed` state is reached. It calls
	/// the `after_job` method of each step in the pipeline with the output.
	Finalizing(Pin<Box<dyn Future<Output = PipelineOutput<P>> + Send>>),

	/// The pipeline is currently preparing to execute the next step.
	/// We are in this state only for a brief moment inside the `poll` method
	/// and it will never be seen by the `run_step` method.
	PreparingStep,
}

/// This is a hack to allow cloning of the `PayloadBuilderError`.
/// Because the same result of this future is cached and reuesed
/// in the `PayloadJob`, we need to be able to clone the error so
/// we can use it with `futures::Shared`
#[derive(Debug)]
pub(super) struct ClonablePayloadBuilderError(PayloadBuilderError);

impl Clone for ClonablePayloadBuilderError {
	fn clone(&self) -> Self {
		Self(clone_payload_error(&self.0))
	}
}

pub(crate) fn clone_payload_error(
	error: &PayloadBuilderError,
) -> PayloadBuilderError {
	match error {
		PayloadBuilderError::MissingParentHeader(fixed_bytes) => {
			PayloadBuilderError::MissingParentHeader(*fixed_bytes)
		}
		PayloadBuilderError::MissingParentBlock(fixed_bytes) => {
			PayloadBuilderError::MissingParentBlock(*fixed_bytes)
		}
		PayloadBuilderError::ChannelClosed => PayloadBuilderError::ChannelClosed,
		PayloadBuilderError::MissingPayload => PayloadBuilderError::MissingPayload,
		PayloadBuilderError::Internal(reth_error) => {
			PayloadBuilderError::other(WrappedErrorMessage(reth_error.to_string()))
		}

		PayloadBuilderError::EvmExecutionError(error)
		| PayloadBuilderError::Other(error) => {
			PayloadBuilderError::other(WrappedErrorMessage(error.to_string()))
		}
	}
}

impl From<Box<dyn core::error::Error>> for ClonablePayloadBuilderError {
	fn from(error: Box<dyn core::error::Error>) -> Self {
		Self(PayloadBuilderError::other(WrappedErrorMessage(
			error.to_string(),
		)))
	}
}

impl From<PayloadBuilderError> for ClonablePayloadBuilderError {
	fn from(error: PayloadBuilderError) -> Self {
		Self(error)
	}
}

impl From<ClonablePayloadBuilderError> for PayloadBuilderError {
	fn from(error: ClonablePayloadBuilderError) -> Self {
		error.0
	}
}
struct WrappedErrorMessage(String);
impl Display for WrappedErrorMessage {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}
impl Debug for WrappedErrorMessage {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}
impl std::error::Error for WrappedErrorMessage {}
