use {
	metrics::{Counter, Histogram},
	metrics_derive::Metrics,
};

#[derive(Metrics)]
#[metrics(dynamic = true)]
pub struct Metrics {
	/// The total number of times this step's `step` method has been invoked.
	pub invoked_total: Counter,

	/// The total number of times this step's `before_job` method has been
	/// invoked.
	pub before_job_invoked_total: Counter,

	/// The number of times `before_job` has failed.
	pub before_job_failed_total: Counter,

	/// The duration of time spent executing this step's `before_job` method.
	pub before_job_exec_duration_histogram: Histogram,

	/// The cumulative number of milliseconds spent executing this step's
	/// `before_job` method across all runs.
	pub before_job_exec_duration_total_millis: Counter,

	/// The total number of times this step's `after_job` method has been
	/// invoked.
	pub after_job_invoked_total: Counter,

	/// The number of times `after_job` has failed.
	pub after_job_failed_total: Counter,

	/// The duration of time spent executing this step's `after_job` method.
	pub after_job_exec_duration_histogram: Histogram,

	/// The cumulative number of milliseconds spent executing this step's
	/// `after_job` method across all runs.
	pub after_job_exec_duration_total_millis: Counter,

	/// The total number of times this step's `step` method has returned
	/// `ControlFlow::Ok`.
	pub ok_total: Counter,

	/// The total number of times this step's `step` method has returned
	/// `ControlFlow::Break`.
	pub break_total: Counter,

	/// The total number of times this step's `step` method has returned
	/// `ControlFlow::Fail`.
	pub fail_total: Counter,

	/// The duration of time spent executing this step's `step` method.
	pub exec_duration_histogram: Histogram,

	/// The cumulative number of milliseconds spent executing this step's `step`
	/// method across all runs.
	pub exec_duration_total_millis: Counter,
}
