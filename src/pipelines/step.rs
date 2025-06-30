use {
	super::sealed,
	crate::{Platform, Simulated, Static, StepContext},
	core::{
		any::{type_name, Any, TypeId},
		fmt::{self, Debug},
		future::Future,
		pin::Pin,
	},
	futures::FutureExt,
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
};

/// Defines the kind of step of a pipeline.
///
/// There are two kinds of steps:
/// - Static: These steps are executed in a static context, without access to
///   execution results or state manipulation.
/// - Simulated: These steps are executed in a simulated context, with access to
///   execution results and state manipulation capabilities.
///
/// This trait cannot be implemented by users of this library and only the
/// two provided implementations are available.
pub trait StepKind: sealed::Sealed + Debug + Sync + Send + 'static {
	type Payload<P: Platform>: Send + Sync + 'static;
}

pub trait Step<P: Platform>: Send + Sync + 'static {
	type Kind: StepKind;

	/// Gets called every time this step is executed in the pipeline.
	fn step(
		self: Arc<Self>,
		payload: <Self::Kind as StepKind>::Payload<P>,
		ctx: StepContext<P>,
	) -> impl Future<Output = ControlFlow<P, Self::Kind>> + Send + Sync;
}

/// This type is returned from every step in the pipeline and it controls the
/// next action of the pipeline execution.
#[derive(Debug)]
pub enum ControlFlow<P: Platform, S: StepKind> {
	/// Terminate the pipeline execution with an error.
	/// No valid payload will be produced by this pipeline run.
	Fail(PayloadBuilderError),

	/// Stops the pipeline execution and returns the payload.
	///
	/// If the step is inside a `Loop` sub-pipeline, it will leave the loop
	/// and progress to next steps immediately after the loop with the output
	/// carried by this variant.
	Break(S::Payload<P>),

	/// Continues the pipeline execution to the next step with the given payload
	/// version
	Ok(S::Payload<P>),

	/// This step is only valid in a `Loop` sub-pipeline, it will jump to the
	/// first step of the loop with the given payload version.
	Continue(S::Payload<P>),
}

impl<P: Platform, S: StepKind, E: core::error::Error + Send + Sync + 'static>
	From<E> for ControlFlow<P, S>
{
	fn from(value: E) -> Self {
		ControlFlow::Fail(PayloadBuilderError::other(value))
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowKind {
	Ok,
	Continue,
	Break,
	Fail,
}

impl<P: Platform, S: StepKind> ControlFlow<P, S> {
	pub fn try_into_payload(self) -> Result<S::Payload<P>, Self> {
		match self {
			ControlFlow::Ok(payload)
			| ControlFlow::Break(payload)
			| ControlFlow::Continue(payload) => Ok(payload),
			_ => Err(self),
		}
	}

	pub const fn kind(&self) -> ControlFlowKind {
		match self {
			ControlFlow::Ok(_) => ControlFlowKind::Ok,
			ControlFlow::Continue(_) => ControlFlowKind::Continue,
			ControlFlow::Break(_) => ControlFlowKind::Break,
			ControlFlow::Fail(_) => ControlFlowKind::Fail,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KindTag {
	Static,
	Simulated,
}

#[allow(type_alias_bounds)]
type WrappedStepFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		Box<dyn Any + Send + Sync>,
		StepContext<P>,
	) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send + Sync>> + Send>>,
>;

/// Wraps a step in a type-erased manner, allowing it to be stored in a
/// heterogeneous collection of steps inside a pipeline.
pub(crate) struct WrappedStep<P: Platform> {
	instance: Arc<dyn Any + Send + Sync>,
	step_fn: WrappedStepFn<P>,
	mode: KindTag,
	name: &'static str,
}

impl<P: Platform> WrappedStep<P> {
	pub fn new<M: StepKind, S: Step<P, Kind = M>>(step: S) -> Self {
		let step: Arc<dyn Any + Send + Sync> = Arc::new(step);

		// This is the only place we have access to the concrete step type
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
				 payload: Box<dyn Any + Send + Sync>,
				 ctx: StepContext<P>|
				 -> Pin<
					Box<dyn Future<Output = Box<dyn Any + Send + Sync>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					let payload = payload
						.downcast::<M::Payload<P>>()
						.expect("Invalid payload type");
					step
						.step(*payload, ctx)
						.map(|result| Box::new(result) as Box<dyn Any + Send + Sync>)
						.boxed()
				},
			) as WrappedStepFn<P>,
			name: type_name::<S>(),
			mode: if TypeId::of::<M>() == TypeId::of::<Static>() {
				KindTag::Static
			} else if TypeId::of::<M>() == TypeId::of::<Simulated>() {
				KindTag::Simulated
			} else {
				unreachable!("Unsupported StepMode type")
			},
		}
	}

	/// This is invoked from places where we know the kind of the step and
	/// all other concrete types needed to execute the step and consume its
	/// output.
	pub async fn execute<M: StepKind>(
		&self,
		payload: M::Payload<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P, M> {
		let local_step = Arc::clone(&self.instance);

		let payload_box = Box::new(payload) as Box<dyn Any + Send + Sync>;
		let result = (self.step_fn)(local_step, payload_box, ctx).await;
		let result = result
			.downcast::<ControlFlow<P, M>>()
			.expect("Invalid result type");

		*result
	}

	/// Checks if the step is static or simulated.
	pub const fn kind(&self) -> KindTag {
		self.mode
	}

	/// Returns the name of the type that implements this step.
	pub const fn name(&self) -> &'static str {
		self.name
	}
}

impl<P: Platform> Debug for WrappedStep<P> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mode = match self.kind() {
			KindTag::Static => "Static",
			KindTag::Simulated => "Simulated",
		};

		write!(f, "Step {{ name: {:?}, mode: {:?} }}", self.name(), mode)
	}
}

unsafe impl<P: Platform> Send for WrappedStep<P> {}
unsafe impl<P: Platform> Sync for WrappedStep<P> {}
