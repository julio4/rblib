use {
	super::sealed,
	alloc::boxed::Box,
	core::{fmt::Debug, future::Future, pin::Pin, ptr::NonNull},
};

pub trait StepMode: sealed::Sealed + Debug + Sync + Send + 'static {
	type Payload: Send;
	type Context: Send;
}

pub trait Step: Send + Sync + 'static {
	type Mode: StepMode;

	fn step(
		&mut self,
		payload: <Self::Mode as StepMode>::Payload,
		ctx: &mut <Self::Mode as StepMode>::Context,
	) -> impl Future<Output = ControlFlow<Self::Mode>> + Send;
}

#[derive(Debug)]
pub enum ControlFlow<S: StepMode> {
	Fail(Box<dyn core::error::Error>),
	Break(S::Payload),
	Ok(S::Payload),
	Continue(S::Payload),
	Goto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeType {
	Static,
	Simulated,
}

// Function pointer table for type-erased operations
struct StepVTable {
	#[expect(clippy::type_complexity)]
	step_fn: unsafe fn(
		step_ptr: *mut u8,
		payload_ptr: *mut u8,
		ctx_ptr: *mut u8,
	) -> Pin<Box<dyn Future<Output = *mut u8> + Send>>,
	drop_fn: unsafe fn(*mut u8),
	mode: ModeType,
	name: &'static str,
}

pub(crate) struct WrappedStep {
	step_ptr: NonNull<u8>,
	vtable: StepVTable,
}

impl WrappedStep {
	pub fn new<M: StepMode + 'static, S: Step<Mode = M> + 'static>(
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
			mode: if core::any::TypeId::of::<M>()
				== core::any::TypeId::of::<crate::pipeline::Static>()
			{
				ModeType::Static
			} else if core::any::TypeId::of::<M>()
				== core::any::TypeId::of::<crate::pipeline::Simulated>()
			{
				ModeType::Simulated
			} else {
				unreachable!("Unsupported StepMode type")
			},
			name: core::any::type_name::<S>(),
		};

		Self { step_ptr, vtable }
	}

	pub async fn execute<M: StepMode + 'static>(
		&mut self,
		mut payload: M::Payload,
		ctx: &mut M::Context,
	) -> ControlFlow<M> {
		let payload_ptr = &mut payload as *mut M::Payload as *mut u8;
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

	pub const fn mode(&self) -> ModeType {
		self.vtable.mode
	}

	pub const fn name(&self) -> &'static str {
		self.vtable.name
	}
}

unsafe fn step_fn_impl<M: StepMode + 'static, S: Step<Mode = M> + 'static>(
	step_ptr: *mut u8,
	payload_ptr: *mut u8,
	ctx_ptr: *mut u8,
) -> Pin<Box<dyn Future<Output = *mut u8> + Send>>
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
			Box::into_raw(Box::new(control_flow)) as *mut u8
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
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		let mode = match self.mode() {
			ModeType::Static => "Static",
			ModeType::Simulated => "Simulated",
		};

		f.debug_struct("Step")
			.field("name", &self.name())
			.field("mode", &mode)
			.finish()
	}
}

unsafe impl Send for WrappedStep {}
unsafe impl Sync for WrappedStep {}
