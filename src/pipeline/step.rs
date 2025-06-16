use {
	super::sealed,
	core::{pin::Pin, ptr::NonNull},
};

pub trait StepMode: sealed::Sealed + Sync + Send + 'static {
	type Payload;
	type Context;
}

pub trait Step: Send + Sync + 'static {
	type Mode: StepMode;

	fn step(
		&mut self,
		payload: <Self::Mode as StepMode>::Payload,
		ctx: &mut <Self::Mode as StepMode>::Context,
	) -> impl Future<Output = ControlFlow> + Send;
}

pub enum ControlFlow {
	Fail,
	Break,
	Ok,
	Continue,
	Goto,
}
// Function pointer table for type-erased operations
struct StepVTable {
	#[expect(clippy::type_complexity)]
	step_fn: unsafe fn(
		step_ptr: *mut u8,
		payload_ptr: *mut u8,
		ctx_ptr: *mut u8,
	) -> Pin<Box<dyn Future<Output = ControlFlow> + Send>>,
	drop_fn: unsafe fn(*mut u8),
	is_static: bool,
}

pub(crate) struct WrappedStep {
	step_ptr: NonNull<u8>,
	vtable: &'static StepVTable,
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

		// Create vtable for this specific step type
		static VTABLE_STORAGE: std::sync::OnceLock<StepVTable> =
			std::sync::OnceLock::new();
		let vtable = VTABLE_STORAGE.get_or_init(|| StepVTable {
			step_fn: step_fn_impl::<M, S>,
			drop_fn: drop_fn_impl::<S>,
			is_static: std::any::type_name::<M>().ends_with("Static"),
		});

		Self { step_ptr, vtable }
	}

	pub async fn execute<M: StepMode + 'static>(
		&mut self,
		mut payload: M::Payload,
		ctx: &mut M::Context,
	) -> ControlFlow
	where
		M: StepMode + 'static,
	{
		let payload_ptr = &mut payload as *mut M::Payload as *mut u8;
		let ctx_ptr = ctx as *mut M::Context as *mut u8;

		unsafe {
			(self.vtable.step_fn)(self.step_ptr.as_ptr(), payload_ptr, ctx_ptr).await
		}
	}

	pub fn is_static(&self) -> bool {
		self.vtable.is_static
	}
}

unsafe fn step_fn_impl<M: StepMode + 'static, S: Step<Mode = M> + 'static>(
	step_ptr: *mut u8,
	payload_ptr: *mut u8,
	ctx_ptr: *mut u8,
) -> Pin<Box<dyn Future<Output = ControlFlow> + Send>>
where
	M::Payload: 'static,
	M::Context: 'static,
{
	unsafe {
		let step = &mut *(step_ptr as *mut S);
		let payload = std::ptr::read(payload_ptr as *mut M::Payload);
		let ctx = &mut *(ctx_ptr as *mut M::Context);

		Box::pin(step.step(payload, ctx))
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

unsafe impl Send for WrappedStep {}
unsafe impl Sync for WrappedStep {}
