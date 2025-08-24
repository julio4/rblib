use {
	super::*,
	core::{
		cell::{Cell, RefCell},
		sync::atomic::{AtomicU32, Ordering},
		time::Duration,
	},
	metrics::{Counter, Histogram},
	std::collections::HashMap,
};

/// Keeps track of the currently active pipeline execution scope relative to the
/// root top-level pipeline.
///
/// The current scope is determined by the last call to `switch_context`.
#[derive(Debug)]
pub struct RootScope {
	root: Scope,
	current: RefCell<StepPath>,
}

impl RootScope {
	pub fn new<P: Platform>(
		pipeline: &Pipeline<P>,
		block: &BlockContext<P>,
	) -> Self {
		let root = Scope::rooted_at(pipeline, block);
		let current = RefCell::new(StepPath::empty());
		Self { root, current }
	}

	/// Given a path to a step in the pipeline, returns its immediate scope.
	pub fn of(&self, step_path: &StepPath) -> Option<&Scope> {
		self.root.get(&scope_of(step_path))
	}

	/// Called from the pipeline executor when switching between steps.
	/// It detects if the next step is in a different scope and enters and leaves
	/// scopes accordingly. This will leave and enter all intermediate scopes
	/// between the previous and next steps.
	pub fn switch_context(&self, next_step: &StepPath) {
		let next = scope_of(next_step);
		let prev = self.current.replace(next.clone());

		if prev != next {
			// Scope changed. We will need to leave all scopes from `prev`
			// up to the common ancestor, then enter all scopes from the
			// common ancestor to `next`.
			let common = prev.common_ancestor(&next);

			for s in prev.between(&common) {
				self.root.get(&s).expect("scope should exist").leave();
			}

			for s in common.between(&next) {
				self.root.get(&s).expect("scope should exist").enter();
			}
		}
	}

	pub fn enter(&self) {
		self.root.enter();
	}

	pub fn leave(&self) {
		self.root.leave();
	}
}

unsafe impl Send for RootScope {}
unsafe impl Sync for RootScope {}

#[inline]
fn scope_of(step: &StepPath) -> StepPath {
	step.clone().remove_leaf().unwrap_or(StepPath::empty())
}

/// Represents a pipeline execution scope.
///
/// Each pipeline has its scope that may include nested scopes for each nested
/// pipeline. Scopes are used to manage limits and metrics for each pipeline
/// execution. All steps in a pipeline run within the scope of the pipeline that
/// contains it. When a scope is active, then all its parent scopes are active
/// as well.
#[derive(Debug)]
pub struct Scope {
	limits: Limits,
	metrics: Metrics,
	entered: Cell<Option<Instant>>,
	enter_counter: AtomicU32,
	nested: HashMap<usize, Scope>,
}

// public api
impl Scope {
	/// When a scope is active it means that one of its steps (or in its nested
	/// scopes) is currently being executed,
	pub const fn is_active(&self) -> bool {
		self.entered.get().is_some()
	}

	/// Returns the elapsed time since the scope was entered.
	/// This will only return a value if the scope is currently active.
	pub fn elapsed(&self) -> Option<Duration> {
		self.entered.get().map(|start| start.elapsed())
	}

	/// Returns when the scope was entered most recently.
	pub fn started_at(&self) -> Option<Instant> {
		self.entered.get()
	}

	/// Returns the payload limits for steps running within the current scope.
	pub const fn limits(&self) -> &Limits {
		&self.limits
	}
}

// private api
impl Scope {
	/// Initialize the root scope of a pipeline and all its nested scopes. This
	/// should be called on the top-level pipeline.
	fn rooted_at<P: Platform>(
		root: &Pipeline<P>,
		block: &BlockContext<P>,
	) -> Self {
		let metrics = Metrics::with_scope(&format!("{}_pipeline", root.name()));
		let limits = root.limits().map_or_else(
			|| P::DefaultLimits::default().create(block, None),
			|limits| limits.create(block, None),
		);

		let mut nested = HashMap::new();
		for (ix, step) in root.steps().iter().enumerate() {
			if let StepOrPipeline::Pipeline(_, inner) = step {
				let path = StepPath::step(ix);
				let scope = Self::inner(inner, block, &path, root.name(), &limits);
				nested.insert(path.leaf(), scope);
			}
		}

		Self {
			limits,
			metrics,
			nested,
			entered: Cell::new(None),
			enter_counter: AtomicU32::new(0),
		}
	}

	fn enter(&self) {
		assert!(!self.is_active(), "Scope is already active");
		self.entered.set(Some(Instant::now()));
		self.enter_counter.fetch_add(1, Ordering::Relaxed);
		self.metrics.iter_count_total.increment(1);
	}

	fn leave(&self) {
		assert!(self.is_active(), "Scope is not active");
		// leave any active nested scope
		for nested in self.nested.values() {
			if nested.is_active() {
				nested.leave();
			}
		}

		let duration = self
			.elapsed()
			.expect("Scope must be entered before leaving");

		#[expect(clippy::cast_possible_truncation)]
		self
			.metrics
			.exec_duration_total_millis
			.increment(duration.as_millis() as u64);

		self.metrics.exec_duration_histogram.record(duration);
		self.entered.replace(None);
	}

	/// Returns the scope with a given path.
	fn get(&self, step: &StepPath) -> Option<&Scope> {
		if *step == StepPath::empty() {
			return Some(self);
		}

		let next = step.root().leaf();
		let suffix = step.clone().remove_root().unwrap_or(StepPath::empty());
		self.nested.get(&next)?.get(&suffix)
	}

	/// Internally construct nested scopes. Called from `rooted_at`.
	fn inner<P: Platform>(
		local: &Pipeline<P>,
		block: &BlockContext<P>,
		path: &StepPath,
		root_name: &str,
		enclosing: &Limits,
	) -> Self {
		let limits = local
			.limits()
			.map_or_else(
				|| enclosing.clone(),
				|limits| limits.create(block, Some(enclosing)),
			)
			.clamp(enclosing);

		let scope_name = format!("{root_name}_pipeline_{path}");
		let metrics = Metrics::with_scope(&scope_name);

		let mut nested = HashMap::new();
		for (ix, step) in local.steps().iter().enumerate() {
			if let StepOrPipeline::Pipeline(_, inner) = step {
				let path = path.clone().concat(StepPath::step(ix));
				let scope = Self::inner(inner, block, &path, root_name, &limits);
				nested.insert(path.leaf(), scope);
			}
		}

		Self {
			limits,
			metrics,
			nested,
			entered: Cell::new(None),
			enter_counter: AtomicU32::new(0),
		}
	}
}

impl Drop for Scope {
	fn drop(&mut self) {
		if self.is_active() {
			self.leave();
		}

		let iter_count = self.enter_counter.load(Ordering::Relaxed);
		self.metrics.iter_count_histogram.record(iter_count);
	}
}

unsafe impl Send for Scope {}
unsafe impl Sync for Scope {}

#[derive(MetricsSet)]
pub struct Metrics {
	/// Histogram of the number of iterations.
	pub iter_count_histogram: Histogram,

	/// Total number of iterations across all payload jobs.
	pub iter_count_total: Counter,

	/// Histogram of the execution duration.
	pub exec_duration_histogram: Histogram,

	/// Total execution duration across all payload jobs.
	pub exec_duration_total_millis: Counter,
}
