use {
	super::sealed,
	crate::{Platform, Simulated, Static, StepContext},
	core::{
		any::{type_name, TypeId},
		fmt::{self, Debug},
		future::Future,
		marker::PhantomData,
		pin::Pin,
		ptr::NonNull,
	},
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

pub trait Step: Send + Sync + 'static {
	type Kind: StepKind;

	/// Gets called every time this step is executed in the pipeline.
	fn step<P: Platform>(
		&mut self,
		payload: <Self::Kind as StepKind>::Payload<P>,
		ctx: &StepContext<P>,
	) -> impl Future<Output = ControlFlow<P, Self::Kind>> + Send + Sync;
}

/// This type is returned from every step in the pipeline and it controls the
/// next action of the pipeline execution.
#[derive(Debug)]
pub enum ControlFlow<P: Platform, S: StepKind> {
	/// Terminate the pipeline execution with an error.
	/// No valid payload will be produced by this pipeline run.
	Fail(Box<dyn core::error::Error>),

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

impl<P: Platform, S: StepKind, E: core::error::Error + 'static> From<E>
	for ControlFlow<P, S>
{
	fn from(value: E) -> Self {
		ControlFlow::Fail(Box::new(value))
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KindTag {
	Static,
	Simulated,
}

/// Function pointer table for type-erased operations.
///
/// This is used internally in the pipeline implementation to abstract the
/// underlying developer friendly typed version of the step.
#[derive(Clone)]
struct StepVTable {
	#[expect(clippy::type_complexity)]
	step_fn: unsafe fn(
		step_ptr: *const u8,
		payload_ptr: *const u8,
		ctx_ptr: *const u8,
	)
		-> Pin<Box<dyn Future<Output = *const u8> + Send + Sync>>,
	drop_fn: unsafe fn(*mut u8),
	mode: KindTag,
	name: &'static str,
}

/// Wraps a step in a type-erased manner, allowing it to be stored in a
/// heterogeneous collection of steps inside a pipeline.
pub(crate) struct WrappedStep<P: Platform> {
	step_ptr: NonNull<u8>,
	vtable: StepVTable,
	_p: PhantomData<P>,
}

impl<P: Platform> Clone for WrappedStep<P> {
	fn clone(&self) -> Self {
		Self {
			step_ptr: self.step_ptr,
			vtable: self.vtable.clone(),
			_p: PhantomData,
		}
	}
}

impl<P: Platform> WrappedStep<P> {
	pub fn new<M: StepKind, S: Step<Kind = M>>(step: S) -> Self {
		let boxed_step = Box::new(step);
		let step_ptr = NonNull::new(Box::into_raw(boxed_step) as *mut u8).unwrap();

		let vtable = StepVTable {
			step_fn: step_fn_impl::<P, M, S>,
			drop_fn: drop_fn_impl::<S>,
			mode: if TypeId::of::<M>() == TypeId::of::<Static>() {
				KindTag::Static
			} else if TypeId::of::<M>() == TypeId::of::<Simulated>() {
				KindTag::Simulated
			} else {
				unreachable!("Unsupported StepMode type")
			},
			name: type_name::<S>(),
		};

		Self {
			step_ptr,
			vtable,
			_p: PhantomData,
		}
	}

	pub async fn execute<M: StepKind>(
		&self,
		payload: M::Payload<P>,
		ctx: &StepContext<P>,
	) -> ControlFlow<P, M> {
		let payload_ptr = &payload as *const M::Payload<P> as *const u8;
		let ctx_ptr = ctx as *const StepContext<P> as *mut u8;

		unsafe {
			let result_ptr =
				(self.vtable.step_fn)(self.step_ptr.as_ptr(), payload_ptr, ctx_ptr)
					.await;
			// Cast the type-erased pointer back to the concrete ControlFlow<M>
			let control_flow = Box::from_raw(result_ptr as *mut ControlFlow<P, M>);
			*control_flow
		}
	}

	pub const fn kind(&self) -> KindTag {
		self.vtable.mode
	}

	pub const fn name(&self) -> &'static str {
		self.vtable.name
	}
}

unsafe fn step_fn_impl<P: Platform, M: StepKind, S: Step<Kind = M>>(
	step_ptr: *const u8,
	payload_ptr: *const u8,
	ctx_ptr: *const u8,
) -> Pin<Box<dyn Future<Output = *const u8> + Send + Sync>> {
	unsafe {
		let step = &mut *(step_ptr as *mut S);
		let payload = core::ptr::read(payload_ptr as *mut M::Payload<P>);
		let ctx = &*(ctx_ptr as *const StepContext<P>);

		Box::pin(async move {
			let control_flow = step.step::<P>(payload, ctx).await;
			// Box the result and return as type-erased pointer
			Box::into_raw(Box::new(control_flow)) as *const u8
		}) as Pin<Box<dyn Future<Output = *const u8> + Send + Sync>>
	}
}

unsafe fn drop_fn_impl<S>(step_ptr: *mut u8) {
	let _ = unsafe { Box::from_raw(step_ptr as *mut S) };
}

impl<P: Platform> Drop for WrappedStep<P> {
	fn drop(&mut self) {
		unsafe {
			(self.vtable.drop_fn)(self.step_ptr.as_ptr());
		}
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
