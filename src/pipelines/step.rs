use {
	super::sealed,
	crate::{Simulated, Static},
	core::{
		any::{type_name, TypeId},
		fmt::{self, Debug},
		future::Future,
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
	type Payload: Send;
	type Context: Send;
}

pub trait Step: Send + Sync + 'static {
	type Kind: StepKind;

	fn step(
		&mut self,
		payload: <Self::Kind as StepKind>::Payload,
		ctx: &mut <Self::Kind as StepKind>::Context,
	) -> impl Future<Output = ControlFlow<Self::Kind>> + Send;
}

/// This type is returned from every step in the pipeline and it controls the
/// next action of the pipeline execution.
#[derive(Debug)]
pub enum ControlFlow<S: StepKind> {
	/// Terminate the pipeline execution with an error.
	/// No valid payload will be produced by this pipeline run.
	Fail(Box<dyn core::error::Error>),

	/// Stops the pipeline execution and returns the payload.
	///
	/// If the step is inside a `Loop` sub-pipeline, it will leave the loop
	/// and progress to next steps immediately after the loop with the output
	/// carried by this variant.
	Break(S::Payload),

	/// Continues the pipeline execution to the next step with the given payload
	/// version
	Ok(S::Payload),

	/// This step is only valid in a `Loop` sub-pipeline, it will jump to the
	/// first step of the loop with the given payload version.
	Continue(S::Payload),
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
		ctx_ptr: *mut u8,
	) -> Pin<Box<dyn Future<Output = *const u8> + Send>>,
	drop_fn: unsafe fn(*mut u8),
	mode: KindTag,
	name: &'static str,
}

/// Wraps a step in a type-erased manner, allowing it to be stored in a
/// heterogeneous collection of steps inside a pipeline.
#[derive(Clone)]
pub(crate) struct WrappedStep {
	step_ptr: NonNull<u8>,
	vtable: StepVTable,
}

impl WrappedStep {
	pub fn new<M: StepKind + 'static, S: Step<Kind = M> + 'static>(
		step: S,
	) -> Self
	where
		M::Payload: 'static,
		M::Context: 'static,
	{
		let boxed_step = Box::new(step);
		let step_ptr = NonNull::new(Box::into_raw(boxed_step) as *mut u8).unwrap();

		let vtable = StepVTable {
			step_fn: step_fn_impl::<M, S>,
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

		Self { step_ptr, vtable }
	}

	pub async fn execute<M: StepKind + 'static>(
		&self,
		payload: M::Payload,
		ctx: &mut M::Context,
	) -> ControlFlow<M> {
		let payload_ptr = &payload as *const M::Payload as *const u8;
		let ctx_ptr = ctx as *mut M::Context as *mut u8;

		unsafe {
			let result_ptr =
				(self.vtable.step_fn)(self.step_ptr.as_ptr(), payload_ptr, ctx_ptr)
					.await;
			// Cast the type-erased pointer back to the concrete ControlFlow<M>
			let control_flow = Box::from_raw(result_ptr as *mut ControlFlow<M>);
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

unsafe fn step_fn_impl<M: StepKind + 'static, S: Step<Kind = M> + 'static>(
	step_ptr: *const u8,
	payload_ptr: *const u8,
	ctx_ptr: *mut u8,
) -> Pin<Box<dyn Future<Output = *const u8> + Send>>
where
	M::Payload: 'static,
	M::Context: 'static,
{
	unsafe {
		let step = &mut *(step_ptr as *mut S);
		let payload = core::ptr::read(payload_ptr as *mut M::Payload);
		let ctx = &mut *(ctx_ptr as *mut M::Context);

		Box::pin(async move {
			let control_flow = step.step(payload, ctx).await;
			// Box the result and return as type-erased pointer
			Box::into_raw(Box::new(control_flow)) as *const u8
		})
	}
}

unsafe fn drop_fn_impl<S>(step_ptr: *mut u8) {
	let _ = unsafe { Box::from_raw(step_ptr as *mut S) };
}

impl Drop for WrappedStep {
	fn drop(&mut self) {
		unsafe {
			(self.vtable.drop_fn)(self.step_ptr.as_ptr());
		}
	}
}

impl Debug for WrappedStep {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mode = match self.kind() {
			KindTag::Static => "Static",
			KindTag::Simulated => "Simulated",
		};

		f.debug_struct("Step")
			.field("name", &self.name())
			.field("mode", &mode)
			.finish()
	}
}

unsafe impl Send for WrappedStep {}
unsafe impl Sync for WrappedStep {}
