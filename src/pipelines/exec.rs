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
			step::{ControlFlowKind, KindTag},
		},
		*,
	},
	core::{
		fmt::{Debug, Display},
		pin::Pin,
		task::{Context, Poll},
	},
	futures::FutureExt,
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
	tracing::debug,
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
	cursor: Cursor<P>,
	context: ExecContext<P, Provider, Pool>,
}

impl<
		P: Platform,
		Provider: traits::ProviderBounds<P>,
		Pool: traits::PoolBounds<P>,
	> PipelineExecutor<P, Provider, Pool>
{
	pub fn run(
		pipeline: Arc<Pipeline<P>>,
		block: BlockContext<P>,
		service: Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self {
		let cursor = match pipeline.first_step_path() {
			Some(path) => {
				let step = pipeline.step_by_path(path.as_slice()).expect(
					"Step path is unreachable. This is a bug in the pipeline executor \
					 implementation.",
				);

				let input = match step.kind() {
					// If the step is static, we can use the static context and payload.
					KindTag::Static => StepInput::Static(StaticPayload::<P>::default()),
					// If the step is simulated, we can use the simulated context and
					// payload.
					KindTag::Simulated => StepInput::Simulated(block.start()),
				};

				Cursor::BeforeStep(path, input)
			}
			None => {
				// If the pipeline is empty, we can build an empty payload and complete
				// the pipeline immediately. There is nothing to execute.
				debug!(
					"empty pipeline, building empty payloads for attributes: {:?}",
					block.attributes()
				);

				Cursor::<P>::Completed(
					P::construct_payload(
						&block,
						Vec::new(),
						service.pool(),
						service.provider(),
					)
					.map_err(ClonablePayloadBuilderError),
				)
			}
		};

		Self {
			cursor,
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
	/// This method creates a future that encapsulates the execution an an async
	/// step.
	///
	/// It will prepare the step context and all the information a pipeline step
	/// needs to execute, then create a future object that will be stored in the
	/// cursor and polled whenever the executor is polled.
	fn execute_step(
		&self,
		path: &[usize],
		input: StepInput<P>,
	) -> Pin<Box<dyn Future<Output = StepOutput<P>> + Send>> {
		let block = self.context.block.clone();
		let service = Arc::clone(&self.context.service);
		let pipeline = Arc::clone(&self.context.pipeline);
		let step = Arc::clone(pipeline.step_by_path(path).expect(
			"Step path is unreachable. This is a bug in the pipeline executor \
			 implementation.",
		));

		let limits = match pipeline.limits() {
			Some(limits) => limits.create(&block, None),
			None => P::DefaultLimits::default().create(&block, None),
		};

		let context = StepContext::new(block, service, limits);

		async move {
			match (step.kind(), input) {
				(KindTag::Static, StepInput::Static(payload)) => {
					step
						.execute::<Static>(payload, context)
						.map(StepOutput::Static)
						.await
				}
				(KindTag::Simulated, StepInput::Simulated(payload)) => {
					step
						.execute::<Simulated>(payload, context)
						.map(StepOutput::Simulated)
						.await
				}
				(_, _) => {
					unreachable!(
						"Step kind and input type mismatch. This is a bug in the \
						 PipelineExecutor implementation."
					)
				}
			}
		}
		.boxed()
	}

	/// This method handles the control flow of the pipeline execution.
	///
	/// Once a step is executed it determines the next step to execute based on
	/// the output of the step, the current cursor state and the pipeline
	/// structure.
	///
	/// This method also handles the transition between static and simulated
	/// steps.
	fn advance_cursor(
		&self,
		current_step_path: &[usize],
		output: StepOutput<P>,
	) -> Cursor<P> {
		// if the step output is an error, terminate the pipeline execution
		// immediately.
		let output = match output.try_into_fail() {
			Ok(error) => return Cursor::Completed(Err(error.into())),
			Err(output) => output,
		};

		// top-level pipeline
		let root = &self.context.pipeline;

		// get the enclosing pipeline that contains the current step.
		let (pipeline, behavior) =
			root.pipeline_for_step(current_step_path).expect(
				"Step path is unreachable. This is a bug in the pipeline executor \
				 implementation.",
			);

		// the index of the current step in its enclosing pipeline.
		let step_index = *current_step_path.last().expect(
			"Step path is unreachable. This is a bug in the pipeline executor \
			 implementation.",
		);

		let is_last_step = step_index == pipeline.steps().len() - 1;

		let next_step_path = || {
			let step_index_position = current_step_path.len() - 1;
			let mut next_path = current_step_path.to_vec();
			next_path[step_index_position] += 1;
			next_path
		};

		let first_step_path = || {
			let step_index_position = current_step_path.len() - 1;
			let mut next_path = current_step_path.to_vec();
			next_path[step_index_position] = 0;
			next_path
		};

		let flow = output.kind();

		let create_cursor = |next_path: Vec<usize>| match self
			.prepare_step_input(&next_path, output)
		{
			Ok(input) => Cursor::BeforeStep(next_path, input),
			Err(error) => Cursor::Completed(Err(ClonablePayloadBuilderError(
				PayloadBuilderError::other(WrappedErrorMessage(error.to_string())),
			))),
		};

		match flow {
			ControlFlowKind::Ok => {
				if is_last_step {
					match behavior {
						// terminate current sub-pipeline
						Once => todo!("Ok/last step in Once sub-pipeline"),
						Loop => {
							// run first step in the sub-pipeline
							create_cursor(first_step_path())
						}
					}
				} else {
					// run next step in the sub-pipeline
					create_cursor(next_step_path())
				}
			}
			ControlFlowKind::Continue => {
				match behavior {
					Once => {
						if is_last_step {
							// terminate current sub-pipeline
							todo!("Continue/last step in Once sub-pipeline");
						} else {
							// run next step in pipeline
							create_cursor(next_step_path())
						}
					}
					Loop => {
						// run first step in the sub-pipeline
						create_cursor(first_step_path())
					}
				}
			}
			ControlFlowKind::Break => {
				todo!("ControlFlowKind::Break not implemented yet")
			}
			ControlFlowKind::Fail => unreachable!(
				"Failures already handled before this point, this is a bug in the \
				 PipelineExecutor implementation."
			),
		}
	}

	/// This method handles the transition between the output of one step to the
	/// input of the next step.
	///
	/// todo: optimize this and apply caching
	fn prepare_step_input(
		&self,
		next_path: &[usize],
		previous: StepOutput<P>,
	) -> Result<StepInput<P>, CheckpointError<P>> {
		let step = self.context.pipeline.step_by_path(next_path).expect(
			"Step path is unreachable. This is a bug in the pipeline executor \
			 implementation.",
		);

		match (step.kind(), previous) {
			(KindTag::Static, StepOutput::Static(payload)) => {
				// If the next step is static, we can use the static payload as is.
				Ok(StepInput::Static(payload.try_into_payload().expect(
					"Step output is not a static payload. This is a bug in the \
					 PipelineExecutor implementation.",
				)))
			}
			(KindTag::Simulated, StepOutput::Simulated(payload)) => {
				// If the next step is simulated, we can use the simulated payload as
				// is.
				Ok(StepInput::Simulated(payload.try_into_payload().expect(
					"Step output is not a simulated payload. This is a bug in the \
					 PipelineExecutor implementation.",
				)))
			}
			(KindTag::Simulated, StepOutput::Static(payload)) => {
				let transactions = payload.try_into_payload().expect(
					"Step output is not a simulated payload. This is a bug in the \
					 PipelineExecutor implementation.",
				);
				Ok(StepInput::Simulated(
					transactions
						.into_iter()
						.try_fold(self.context.block.start(), |acc, tx| acc.apply(tx))?,
				))
			}
			(KindTag::Static, StepOutput::Simulated(payload)) => {
				let payload = payload.try_into_payload().expect(
					"Step output is not a simulated payload. This is a bug in the \
					 PipelineExecutor implementation.",
				);
				Ok(StepInput::Static(
					payload.history().transactions().cloned().collect(),
				))
			}
		}
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

			let running_future = executor.execute_step(&path, input);
			executor.cursor = Cursor::InProgress(path, running_future);
			cx.waker().wake_by_ref(); // tell the async runtime to poll again
		}

		if let Cursor::InProgress(ref path, ref mut pinned_future) = executor.cursor
		{
			// If the cursor is in the InProgress state, we need to poll the future
			// to see if it has completed.
			if let Poll::Ready(output) = pinned_future.as_mut().poll_unpin(cx) {
				// step has completed, we can advance the cursor
				executor.cursor = executor.advance_cursor(path, output);
				cx.waker().wake_by_ref(); // tell the async runtime to poll again
			}
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
	/// The `Vec<usize>` contains the indices of the steps that will be executed
	/// in the next iteration. It's a vector because the pipeline can have
	/// nested pipelines, each nesting level will add its own index to the
	/// vector.
	BeforeStep(Vec<usize>, StepInput<P>),

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
		Pin<Box<dyn Future<Output = StepOutput<P>> + Send>>,
	),

	/// The pipeline is currently preparing to execute the next step.
	/// We are in this state only for a brief moment inside the `poll` method
	/// and it will never be seen by the `run_step` method.
	PreparingStep,
}

#[derive(Debug)]
enum StepInput<P: Platform> {
	Static(StaticPayload<P>),
	Simulated(SimulatedPayload<P>),
}

#[derive(Debug)]
enum StepOutput<P: Platform> {
	Static(ControlFlow<P, Static>),
	Simulated(ControlFlow<P, Simulated>),
}

impl<P: Platform> StepOutput<P> {
	pub fn try_into_fail(self) -> Result<PayloadBuilderError, Self> {
		match self {
			StepOutput::Static(ControlFlow::Fail(error))
			| StepOutput::Simulated(ControlFlow::Fail(error)) => Ok(error),
			_ => Err(self),
		}
	}

	pub const fn kind(&self) -> ControlFlowKind {
		match self {
			StepOutput::Static(flow) => flow.kind(),
			StepOutput::Simulated(flow) => flow.kind(),
		}
	}
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
			PayloadBuilderError::Internal(reth_error) => Self(
				PayloadBuilderError::other(WrappedErrorMessage(reth_error.to_string())),
			),
			PayloadBuilderError::EvmExecutionError(error) => Self(
				PayloadBuilderError::other(WrappedErrorMessage(error.to_string())),
			),
			PayloadBuilderError::Other(error) => Self(PayloadBuilderError::other(
				WrappedErrorMessage(error.to_string()),
			)),
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
