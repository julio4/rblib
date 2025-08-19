use {
	crate::{alloy::eips::eip7840::BlobParams, prelude::*},
	core::time::Duration,
	std::time::Instant,
};

pub trait LimitsFactory<P: Platform>: Send + Sync + 'static {
	/// Configure the limits for the block payload under construction.
	///
	/// As an input this method takes the block context that we're producing a
	/// payload for and optionally any limits imposed by enclosing pipelines.
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits;
}

#[derive(Debug, Clone)]
pub struct Limits {
	/// The maximum cumulative gas that can be used in the block.
	/// This includes all transactions, epilogues, prologues, and other
	/// gas-consuming operations.
	pub gas_limit: u64,

	/// Limits for blob transactions in the block.
	pub blob_params: Option<BlobParams>,

	/// The maximum number of transactions that can be included in the block.
	///
	/// This is not a standard known ethereum limit, however it can be imposed by
	/// custom limits factories.
	pub max_transactions: Option<usize>,

	/// The time a pipeline is allowed to spend on execution.
	pub deadline: Option<Duration>,
}

impl Limits {
	pub fn with_gas_limit(gas_limit: u64) -> Self {
		Self {
			gas_limit,
			blob_params: None,
			deadline: None,
			max_transactions: None,
		}
	}

	#[must_use]
	pub fn with_blob_params(mut self, blob_params: BlobParams) -> Self {
		self.blob_params = Some(blob_params);
		self
	}

	#[must_use]
	pub fn with_max_transactions(mut self, max_transactions: usize) -> Self {
		self.max_transactions = Some(max_transactions);
		self
	}

	#[must_use]
	pub fn with_deadline_at(mut self, deadline: Instant) -> Self {
		self.deadline = Some(deadline.duration_since(Instant::now()));
		self
	}

	#[must_use]
	pub fn with_deadline(mut self, deadline: Duration) -> Self {
		self.deadline = Some(deadline);
		self
	}

	#[must_use]
	pub fn clamp(self, other: &Self) -> Self {
		Self {
			gas_limit: self.gas_limit.min(other.gas_limit),
			blob_params: match (self.blob_params, other.blob_params) {
				(Some(self_params), Some(other_params)) => Some(BlobParams {
					target_blob_count: self_params
						.target_blob_count
						.min(other_params.target_blob_count),
					max_blob_count: self_params
						.max_blob_count
						.min(other_params.max_blob_count),
					update_fraction: self_params
						.update_fraction
						.min(other_params.update_fraction),
					min_blob_fee: self_params.min_blob_fee.min(other_params.min_blob_fee),
					max_blobs_per_tx: self_params
						.max_blobs_per_tx
						.min(other_params.max_blobs_per_tx),
				}),
				_ => self.blob_params.or(other.blob_params),
			},
			max_transactions: self.max_transactions.min(other.max_transactions),
			deadline: self.deadline.min(other.deadline),
		}
	}
}

/// Convinience trait that allows API users to either used a factory type or a
/// concrete limits value in [`Pipeline::with_limits`].
pub trait IntoLimitsFactory<P: Platform, Marker = ()> {
	/// Convert the type into a limits factory.
	fn into_limits_factory(self) -> impl LimitsFactory<P>;
}

impl<T: LimitsFactory<P>, P: Platform> IntoLimitsFactory<P, Variant<0>> for T {
	fn into_limits_factory(self) -> impl LimitsFactory<P> {
		self
	}
}

impl<P: Platform> IntoLimitsFactory<P, Variant<1>> for Limits {
	fn into_limits_factory(self) -> impl LimitsFactory<P> {
		struct FixedLimits(Limits);
		impl<P: Platform> LimitsFactory<P> for FixedLimits {
			fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
				self.0.clone()
			}
		}

		FixedLimits(self)
	}
}

#[cfg(test)]
mod tests {
	use {
		super::*,
		crate::test_utils::*,
		core::{
			num::NonZero,
			sync::atomic::{AtomicU32, Ordering},
		},
		std::sync::Arc,
	};

	assert_is_dyn_safe!(LimitsFactory<P>, P: Platform);

	#[derive(Debug)]
	struct TestStep {
		must_run: bool,
		iter: AtomicU32,
		break_after: AtomicU32,

		minimum_iterations: Option<u32>,
		maximum_iterations: Option<u32>,
		expected_gas_limit: Option<u64>,
		expected_deadline: Option<Duration>,
		sleep_on_step: Option<Duration>,
	}

	impl Default for TestStep {
		fn default() -> Self {
			Self {
				must_run: false,
				iter: AtomicU32::new(0),
				break_after: AtomicU32::new(1),
				expected_gas_limit: None,
				expected_deadline: None,
				minimum_iterations: None,
				maximum_iterations: None,
				sleep_on_step: None,
			}
		}
	}

	impl TestStep {
		#[allow(dead_code)]
		pub fn break_after(self, iterations: u32) -> Self {
			self.break_after.store(iterations, Ordering::Relaxed);
			self
		}

		pub fn expect_gas_limit(mut self, gas_limit: u64) -> Self {
			self.expected_gas_limit = Some(gas_limit);
			self
		}

		pub fn expect_deadline(mut self, deadline: Duration) -> Self {
			self.expected_deadline = Some(deadline);
			self
		}

		pub fn expect_minimum_iterations(mut self, iterations: u32) -> Self {
			self.minimum_iterations = Some(iterations);
			self
		}

		pub fn expect_maximum_iterations(mut self, iterations: u32) -> Self {
			self.maximum_iterations = Some(iterations);
			self
		}

		pub fn sleep_on_step(mut self, duration: Duration) -> Self {
			self.sleep_on_step = Some(duration);
			self
		}

		#[track_caller]
		pub fn must_run(mut self) -> Self {
			self.must_run = true;
			self
		}
	}

	impl Drop for TestStep {
		fn drop(&mut self) {
			assert!(
				!self.must_run || self.iter.load(Ordering::Relaxed) > 0,
				"Step was expected to run, but did not."
			);

			if let Some(minimum_iterations) = self.minimum_iterations {
				assert!(
					self.iter.load(Ordering::Relaxed) >= minimum_iterations,
					"Expected step to run at least {minimum_iterations} times, but ran \
					 {} times.",
					self.iter.load(Ordering::Relaxed)
				);
			}

			if let Some(maximum_iterations) = self.maximum_iterations {
				assert!(
					self.iter.load(Ordering::Relaxed) <= maximum_iterations,
					"Expected step to run at most {maximum_iterations} times, but ran \
					 {} times.",
					self.iter.load(Ordering::Relaxed)
				);
			}
		}
	}

	impl<P: Platform> Step<P> for TestStep {
		async fn step(
			self: Arc<Self>,
			payload: Checkpoint<P>,
			ctx: StepContext<P>,
		) -> ControlFlow<P> {
			if let Some(sleep_duration) = self.sleep_on_step {
				tokio::time::sleep(sleep_duration).await;
			}

			let break_after = self.break_after.load(Ordering::Relaxed);
			if self.iter.fetch_add(1, Ordering::Relaxed) >= break_after {
				return ControlFlow::Break(payload);
			}

			if ctx.deadline_reached() {
				return ControlFlow::Break(payload);
			}

			if let Some(expected_gas_limit) = self.expected_gas_limit {
				assert_eq!(
					ctx.limits().gas_limit,
					expected_gas_limit,
					"Expected gas limit to be {expected_gas_limit}, but got {}",
					ctx.limits().gas_limit
				);
			}

			if let Some(expected_deadline) = self.expected_deadline {
				assert_eq!(
					ctx.limits().deadline,
					Some(expected_deadline),
					"Expected deadline to be {expected_deadline:?}, but got {:?}",
					ctx.limits().deadline
				);
			}

			ControlFlow::Ok(payload)
		}
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn one_level_flat<P: TestablePlatform>() -> eyre::Result<()> {
		let pipeline = Pipeline::<P>::default()
			.with_step(
				TestStep::default()
					.expect_gas_limit(1000)
					.expect_deadline(Duration::from_millis(500))
					.must_run(),
			)
			.with_limits(
				Limits::with_gas_limit(1000).with_deadline(Duration::from_millis(500)),
			);
		P::create_test_node(pipeline).await?.next_block().await?;
		Ok(())
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn scale_fraction<P: TestablePlatform>() -> eyre::Result<()> {
		let pipeline = Pipeline::<P>::default()
			.with_pipeline(
				Once,
				(TestStep::default()
					.expect_gas_limit(500)
					.expect_deadline(Duration::from_millis(250))
					.must_run(),)
					.with_limits(
						Scaled::default()
							.gas(Fraction(1, NonZero::new(2).unwrap()))
							.deadline(Fraction(1, NonZero::new(2).unwrap())),
					),
			)
			.with_limits(
				Limits::with_gas_limit(1000).with_deadline(Duration::from_millis(500)),
			);
		P::create_test_node(pipeline).await?.next_block().await?;
		Ok(())
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn scale_minus<P: TestablePlatform>() -> eyre::Result<()> {
		let pipeline = Pipeline::<P>::default()
			.with_pipeline(
				Once,
				(TestStep::default()
					.expect_gas_limit(900)
					.expect_deadline(Duration::from_millis(400))
					.must_run(),)
					.with_limits(
						Scaled::default()
							.gas(Minus(100))
							.deadline(Minus(Duration::from_millis(100))),
					),
			)
			.with_limits(
				Limits::with_gas_limit(1000).with_deadline(Duration::from_millis(500)),
			);
		P::create_test_node(pipeline).await?.next_block().await?;
		Ok(())
	}

	/// two levels of scaling
	#[rblib_test(Ethereum, Optimism)]
	async fn deadline_approaches<P: TestablePlatform>() -> eyre::Result<()> {
		let pipeline = Pipeline::<P>::default()
			.with_pipeline(
				Once,
				Pipeline::default()
					.with_pipeline(
						Once,
						(TestStep::default()
							.expect_gas_limit(400)
							.expect_deadline(Duration::from_millis(300))
							.expect_minimum_iterations(2)
							.expect_maximum_iterations(3)
							.sleep_on_step(Duration::from_millis(100))
							.must_run(),)
							.with_limits(
								Scaled::default()
									.gas(Minus(100))
									.deadline(Minus(Duration::from_millis(100))),
							),
					)
					.with_limits(
						Scaled::default()
							.gas(Fraction(1, NonZero::new(2).unwrap()))
							.deadline(Fraction(1, NonZero::new(2).unwrap())),
					),
			)
			.with_limits(
				Limits::with_gas_limit(1000).with_deadline(Duration::from_millis(800)),
			);
		P::create_test_node(pipeline).await?.next_block().await?;
		Ok(())
	}
}
