use {
	super::{
		metrics::{Metrics, PerJobCounters},
		name::Name,
	},
	crate::{pipelines::step::InitContext, prelude::*},
	core::{
		any::Any,
		fmt::{self, Debug},
		pin::Pin,
	},
	futures::FutureExt,
	std::{
		sync::{Arc, OnceLock},
		time::Instant,
	},
};

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
		StepContext<P>,
	) -> Pin<
		Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
	>,
>;

/// Defines a type-erased function that points to the after job function inside
/// a concrete step type.
type WrappedAfterJobFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		StepContext<P>,
		Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Pin<
		Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
	>,
>;

/// Defines a type-erased function that points to the setup function inside
/// a concrete step type.
type WrappedSetupFn<P: Platform> = Box<
	dyn Fn(
		Arc<dyn Any + Send + Sync>,
		InitContext<P>,
	) -> Pin<
		Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
	>,
>;

/// Wraps a step in a type-erased manner, allowing it to be stored in a
/// heterogeneous collection of steps inside a pipeline.
pub(crate) struct StepInstance<P: Platform> {
	instance: Arc<dyn Any + Send + Sync>,
	step_fn: WrappedStepFn<P>,
	before_job_fn: WrappedBeforeJobFn<P>,
	after_job_fn: WrappedAfterJobFn<P>,
	setup_fn: WrappedSetupFn<P>,
	name: Name,
	metrics: OnceLock<Metrics>,
	per_job: PerJobCounters,
}

impl<P: Platform> StepInstance<P> {
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
				 ctx: StepContext<P>|
				 -> Pin<
					Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.before_job(ctx).boxed()
				},
			) as WrappedBeforeJobFn<P>,
			after_job_fn: Box::new(
				|step: Arc<dyn Any + Send + Sync>,
				 ctx: StepContext<P>,
				 result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>|
				 -> Pin<
					Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.after_job(ctx, result).boxed()
				},
			) as WrappedAfterJobFn<P>,
			setup_fn: Box::new(
				|step: Arc<dyn Any + Send + Sync>,
				 ctx: InitContext<P>|
				 -> Pin<
					Box<dyn Future<Output = Result<(), PayloadBuilderError>> + Send>,
				> {
					let step = step.downcast::<S>().expect("Invalid step type");
					step.setup(ctx).boxed()
				},
			) as WrappedSetupFn<P>,
			name: Name::new::<S, P>(),
			metrics: OnceLock::new(),
			per_job: PerJobCounters::default(),
		}
	}

	/// This is invoked from places where we know the kind of the step and
	/// all other concrete types needed to execute the step and consume its
	/// output.
	pub async fn step(
		&self,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let metrics = self.metrics.get();

		if let Some(metrics) = metrics {
			metrics.invoked_total.increment(1);
			self.per_job.increment_invocation();
		}

		let started_at = Instant::now();
		let local_step = Arc::clone(&self.instance);
		let result = (self.step_fn)(local_step, payload, ctx).await;

		if let Some(metrics) = metrics {
			let elapsed = started_at.elapsed();
			metrics.exec_duration_histogram.record(elapsed);
			self.per_job.increment_exec_time(elapsed);

			match &result {
				ControlFlow::Ok(_) => metrics.ok_total.increment(1),
				ControlFlow::Break(_) => metrics.break_total.increment(1),
				ControlFlow::Fail(_) => metrics.fail_total.increment(1),
			}
		}

		result
	}

	/// This is invoked once per pipeline run before any steps are executed.
	pub async fn before_job(
		&self,
		ctx: StepContext<P>,
	) -> Result<(), PayloadBuilderError> {
		let metrics = self.metrics.get();

		if let Some(metrics) = metrics {
			metrics.before_job_invoked_total.increment(1);
		}

		let started_at = Instant::now();
		let local_step = Arc::clone(&self.instance);
		let result = (self.before_job_fn)(local_step, ctx).await;

		if let Some(metrics) = metrics {
			let elapsed = started_at.elapsed();
			metrics.before_job_exec_duration_histogram.record(elapsed);
			self.per_job.increment_exec_time(elapsed);

			#[allow(clippy::cast_possible_truncation)]
			metrics
				.before_job_exec_duration_total_millis
				.increment(elapsed.as_millis() as u64);

			if result.is_err() {
				metrics.before_job_failed_total.increment(1);
			}
		}

		result
	}

	/// This is invoked once after the pipeline run has been completed.
	pub async fn after_job(
		&self,
		ctx: StepContext<P>,
		result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Result<(), PayloadBuilderError> {
		let metrics = self.metrics.get();

		if let Some(metrics) = metrics {
			metrics.after_job_invoked_total.increment(1);
			self.per_job.increment_invocation();
		}

		let started_at = Instant::now();
		let local_step = Arc::clone(&self.instance);
		let result = (self.after_job_fn)(local_step, ctx, result).await;

		if let Some(metrics) = metrics {
			let elapsed = started_at.elapsed();
			metrics.after_job_exec_duration_histogram.record(elapsed);
			self.per_job.increment_exec_time(elapsed);

			#[allow(clippy::cast_possible_truncation)]
			metrics
				.after_job_exec_duration_total_millis
				.increment(elapsed.as_millis() as u64);

			// Log the per-job duration.
			let per_job_duration = self.per_job.exec_duration();
			metrics.invoked_per_job.record(self.per_job.invoked_count());
			metrics.exec_duration_per_job.record(per_job_duration);
			#[allow(clippy::cast_possible_truncation)]
			metrics
				.exec_duration_total_millis
				.increment(per_job_duration.as_millis() as u64);
			self.per_job.reset();

			if result.is_err() {
				metrics.after_job_failed_total.increment(1);
			}
		}

		result
	}

	/// This is invoked exactly once when a pipeline is instantiated as a payload
	/// builder service.
	pub async fn setup(
		&self,
		ctx: InitContext<P>,
	) -> Result<(), PayloadBuilderError> {
		let local_step = Arc::clone(&self.instance);
		(self.setup_fn)(local_step, ctx).await
	}

	/// Returns the name of the type that implements this step.
	pub const fn name(&self) -> &str {
		self.name.pretty()
	}

	/// Initializes metrics recording for this step.
	///
	/// The input string is the metric name assigned to this step. This name is
	/// not known before the pipeline instance is fully built and converted into a
	/// service using [`PipelineServiceBuilder`]. It should be called only once.
	pub fn init_metrics(&self, name: &str) {
		// Initialize the metrics name for this step.
		self.name.init_metrics(name);

		// initialize the metrics instance
		self
			.metrics
			.set(Metrics::new(name))
			.expect("Metrics for step {name} already initialized");
	}
}

impl<P: Platform> Debug for StepInstance<P> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Step ({}", self.name.pretty())?;
		if self.name.has_metrics() {
			write!(f, ", metric: {})", self.name.metric())?;
		} else {
			write!(f, ")")?;
		}
		Ok(())
	}
}

unsafe impl<P: Platform> Send for StepInstance<P> {}
unsafe impl<P: Platform> Sync for StepInstance<P> {}
