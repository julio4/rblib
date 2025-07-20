use {
	crate::{Checkpoint, Platform, StepContext, types},
	core::{
		any::{Any, type_name},
		fmt::{self, Debug},
		future::Future,
		pin::Pin,
	},
	futures::FutureExt,
	std::sync::Arc,
};

pub use crate::reth::payload::builder::PayloadBuilderError;

/// This trait defines a step in a pipeline.
///
/// Users of the SDK should implement this trait on their own types to make them
/// usable in a pipeline. A step is a unit of work that can be executed
/// independently and can be composed with other steps in a pipeline.
///
/// Steps receive a `Checkpoint` as an input, which is the state of the payload
/// that has been built so far. The step can then modify this payload, add new
/// transactions, or perform any other operations that are needed to build the
/// final payload. The output of a step is a `ControlFlow` enum that carries a
/// new version of the payload or an error that will terminate the pipeline.
///
/// The `Step` trait is generic over the `Platform` type, which allows it to
/// be used with different platforms (e.g., Ethereum, Optimism, etc.). Steps
/// can be generic over the platform they run on or specialized for a specific
/// platform.
///
/// The instance of the step is long-lived and it's lifetime is equal to the
/// lifetime of the pipeline it is part of. All invocations of the step will
/// repeatedly call into the `step` async function on the same instance.
///
/// There may be multiple instances of the same step in a pipeline.
///
/// A step instance is guaranteed to not be called concurrently by the runtime.
pub trait Step<P: Platform>: Send + Sync + 'static {
	/// Gets called every time this step is executed in the pipeline.
	///
	/// As an input it takes the payload that has been built so far and outputs
	/// a new payload that will be used in the next step of the pipeline, or a
	/// failure that will terminate the pipeline execution.
	fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> impl Future<Output = ControlFlow<P>> + Send + Sync;

	/// This function is called once per new payload job before any steps are
	/// executed. It can be used by steps to perform any optional initialization
	/// of its internal state for a given payload job.
	///
	/// If this function returns an error, the pipeline execution will be
	/// terminated immediately and no steps will be executed.
	fn before_job(
		self: Arc<Self>,
		_: Arc<StepContext<P>>,
	) -> impl Future<Output = Result<(), PayloadBuilderError>> + Send + Sync {
		async { Ok(()) }
	}

	/// This function is called once after all steps in the pipeline have been
	/// executed. It will be called with the outcome of the step execution,
	/// which is either a successful payload or an error.
	///
	/// A failure in this function will invalidate the whole payload job
	/// and will not produce a valid payload.
	fn after_job(
		self: Arc<Self>,
		_: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> impl Future<Output = Result<(), PayloadBuilderError>> + Send + Sync {
		async { Ok(()) }
	}
}

/// This type is returned from every step in the pipeline and it controls the
/// next action of the pipeline execution.
#[derive(Debug)]
pub enum ControlFlow<P: Platform> {
	/// Immediately terminate the pipeline execution with an error and
	/// no valid payload will be produced by the entire hierarchy of pipelines
	/// that contains this step.
	Fail(PayloadBuilderError),

	/// Stops the pipeline execution that contains the step with a payload.
	///
	/// If the step is inside a `Loop` sub-pipeline, it will stop the loop,
	/// run its epilogue (if it exists) and progress to next steps in the parent
	/// pipeline.
	///
	/// Breaking out of a prologue step will not invoke any step in the pipeline,
	/// and jump straight to the epilogue.
	///
	/// Breaking out of an epilogue has the same effect as returning Ok from it,
	/// and will continue the pipeline execution to the next step in the parent
	/// pipeline.
	Break(Checkpoint<P>),

	/// Continues the pipeline execution to the next step with the given payload
	/// version
	Ok(Checkpoint<P>),
}

impl<P: Platform, E: core::error::Error + Send + Sync + 'static> From<E>
	for ControlFlow<P>
{
	fn from(value: E) -> Self {
		ControlFlow::Fail(PayloadBuilderError::other(value))
	}
}

impl<P: Platform> ControlFlow<P> {
	pub fn try_into_payload(self) -> Result<Checkpoint<P>, Self> {
		match self {
			ControlFlow::Ok(payload) | ControlFlow::Break(payload) => Ok(payload),
			ControlFlow::Fail(_) => Err(self),
		}
	}

	pub fn try_into_fail(self) -> Result<PayloadBuilderError, Self> {
		match self {
			ControlFlow::Fail(err) => Ok(err),
			_ => Err(self),
		}
	}

	pub const fn is_break(&self) -> bool {
		matches!(self, ControlFlow::Break(_))
	}

	pub const fn is_fail(&self) -> bool {
		matches!(self, ControlFlow::Fail(_))
	}

	pub const fn is_ok(&self) -> bool {
		matches!(self, ControlFlow::Ok(_))
	}
}

/// Defines a type-erased function that points to the step function inside
/// a concrete step type.
type WrappedStepFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		Checkpoint<P>,
		StepContext<P>,
	) -> Pin<Box<dyn Future<Output = ControlFlow<P>> + Send>>,
>;

/// Defines a type-erased function that points to the before job function inside
/// a concrete step type.
type WrappedBeforeJobFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		Arc<StepContext<P>>,
	) -> Pin<
		Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
	>,
>;

/// Defines a type-erased function that points to the after job function inside
/// a concrete step type.
type WrappedAfterJobFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Pin<
		Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
	>,
>;

/// Wraps a step in a type-erased manner, allowing it to be stored in a
/// heterogeneous collection of steps inside a pipeline.
pub(crate) struct WrappedStep<P: Platform> {
	instance: Arc<dyn Any + Send + Sync>,
	step_fn: WrappedStepFn<P>,
	before_job_fn: WrappedBeforeJobFn<P>,
	after_job_fn: WrappedAfterJobFn<P>,
	name: &'static str,
}

impl<P: Platform> WrappedStep<P> {
	pub fn new<S: Step<P>>(step: S) -> Self {
		let step: Arc<dyn Any + Send + Sync> = Arc::new(step);

		// This is the only place where we have access to the concrete step type
		// information and can leverage that to call into the right step method
		// implementation.
		//
		// Create a boxed closure that will cast from the generic `Any` type
		// to the concrete step type and payload type.
		//
		// Also store the step instance inside an Arc. This arc will be cloned for
		// every step execution, allowing the step to be executed concurrently.
		// Arc will take care of calling the `drop` method on the step
		// instance when the last reference to it is dropped.
		Self {
			instance: step,
			step_fn: Box::new(
				|step: Arc<dyn Any + Send + Sync>,
				 payload: Checkpoint<P>,
				 ctx: StepContext<P>|
				 -> Pin<Box<dyn Future<Output = ControlFlow<P>> + Send>> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.step(payload, ctx).boxed()
				},
			) as WrappedStepFn<P>,
			before_job_fn: Box::new(
				|step: Arc<dyn Any + Send + Sync>,
				 ctx: Arc<StepContext<P>>|
				 -> Pin<
					Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.before_job(ctx).boxed()
				},
			) as WrappedBeforeJobFn<P>,
			after_job_fn: Box::new(
				|step: Arc<dyn Any + Send + Sync>,
				 result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>|
				 -> Pin<
					Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.after_job(result).boxed()
				},
			) as WrappedAfterJobFn<P>,
			name: type_name::<S>(),
		}
	}

	/// This is invoked from places where we know the kind of the step and
	/// all other concrete types needed to execute the step and consume its
	/// output.
	pub async fn execute(
		&self,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let local_step = Arc::clone(&self.instance);
		(self.step_fn)(local_step, payload, ctx).await
	}

	/// This is invoked once per pipeline run before any steps are executed.
	pub async fn before_job(
		&self,
		ctx: Arc<StepContext<P>>,
	) -> Result<(), PayloadBuilderError> {
		let local_step = Arc::clone(&self.instance);
		(self.before_job_fn)(local_step, ctx).await
	}

	/// This is invoked once after the pipeline run has been completed.
	pub async fn after_job(
		&self,
		result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Result<(), PayloadBuilderError> {
		let local_step = Arc::clone(&self.instance);
		(self.after_job_fn)(local_step, result).await
	}

	/// Returns the name of the type that implements this step.
	pub const fn name(&self) -> &'static str {
		self.name
	}
}

impl<P: Platform> Debug for WrappedStep<P> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Step ({})", self.name())
	}
}

unsafe impl<P: Platform> Send for WrappedStep<P> {}
unsafe impl<P: Platform> Sync for WrappedStep<P> {}
