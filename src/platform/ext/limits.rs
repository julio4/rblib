use {
	super::*,
	alloy::eips::eip7840::BlobParams,
	core::{num::NonZero, time::Duration},
};

/// Scales down the limits of the enclosing pipeline (or platform default) by a
/// fixed factor.
#[derive(Default)]
pub struct Scaled {
	gas: Option<ScaleOp<u64>>,
	max_txs: Option<ScaleOp<usize>>,
	max_blob_count: Option<ScaleOp<u64>>,
	max_blobs_per_tx: Option<ScaleOp<u64>>,
	deadline: Option<ScaleOp<Duration>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleOp<T> {
	/// numerator, denominator
	Fraction(u32, NonZero<u32>),
	Minus(T),
	Fixed(T),
	Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction(pub u32, pub NonZero<u32>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Minus<T>(pub T);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixed<T>(pub T);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zero;

impl<T> From<Fraction> for ScaleOp<T> {
	fn from(val: Fraction) -> Self {
		ScaleOp::Fraction(val.0, val.1)
	}
}

impl<T> From<Minus<T>> for ScaleOp<T> {
	fn from(val: Minus<T>) -> Self {
		ScaleOp::Minus(val.0)
	}
}

impl<T> From<Fixed<T>> for ScaleOp<T> {
	fn from(val: Fixed<T>) -> Self {
		ScaleOp::Fixed(val.0)
	}
}

impl<T> From<Zero> for ScaleOp<T> {
	fn from(_: Zero) -> Self {
		ScaleOp::Zero
	}
}

impl<P: Platform> LimitsFactory<P> for Scaled {
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits {
		let mut limits = enclosing
			.cloned()
			.unwrap_or_else(|| P::DefaultLimits::default().create(block, enclosing));

		if let Some(ref op) = self.gas {
			limits.gas_limit = op.apply(limits.gas_limit);
		}

		if let Some(ref op) = self.deadline {
			limits.deadline = op.apply(limits.deadline);
		}

		if let Some(ref op) = self.max_txs {
			limits.max_transactions = op.apply(limits.max_transactions);
		}

		if let Some(ref op) = self.max_blob_count {
			limits.blob_params = limits.blob_params.map(|params| BlobParams {
				max_blob_count: op.apply(params.max_blob_count),
				..params
			});
		}

		if let Some(ref op) = self.max_blobs_per_tx {
			limits.blob_params = limits.blob_params.map(|params| BlobParams {
				max_blobs_per_tx: op.apply(params.max_blobs_per_tx),
				..params
			});
		}

		limits
	}
}

impl Scaled {
	#[must_use]
	pub const fn new() -> Self {
		Self {
			gas: None,
			max_txs: None,
			max_blob_count: None,
			max_blobs_per_tx: None,
			deadline: None,
		}
	}

	pub fn from(&self, limits: &Limits) -> Limits {
		let mut limits = limits.clone();

		if let Some(ref op) = self.gas {
			limits.gas_limit = op.apply(limits.gas_limit);
		}

		if let Some(ref op) = self.deadline {
			limits.deadline = op.apply(limits.deadline);
		}

		if let Some(ref op) = self.max_txs {
			limits.max_transactions = op.apply(limits.max_transactions);
		}

		if let Some(ref op) = self.max_blob_count {
			limits.blob_params = limits.blob_params.map(|params| BlobParams {
				max_blob_count: op.apply(params.max_blob_count),
				..params
			});
		}

		if let Some(ref op) = self.max_blobs_per_tx {
			limits.blob_params = limits.blob_params.map(|params| BlobParams {
				max_blobs_per_tx: op.apply(params.max_blobs_per_tx),
				..params
			});
		}

		limits
	}

	#[must_use]
	pub fn deadline(self, op: impl Into<ScaleOp<Duration>>) -> Self {
		Self {
			deadline: Some(op.into()),
			..self
		}
	}

	#[must_use]
	pub fn gas(self, op: impl Into<ScaleOp<u64>>) -> Self {
		Self {
			gas: Some(op.into()),
			..self
		}
	}

	#[must_use]
	pub fn max_txs(self, op: impl Into<ScaleOp<usize>>) -> Self {
		Self {
			max_txs: Some(op.into()),
			..self
		}
	}

	#[must_use]
	pub fn max_blobs(self, op: impl Into<ScaleOp<u64>>) -> Self {
		Self {
			max_blob_count: Some(op.into()),
			..self
		}
	}

	#[must_use]
	pub fn max_blobs_per_tx(self, op: impl Into<ScaleOp<u64>>) -> Self {
		Self {
			max_blobs_per_tx: Some(op.into()),
			..self
		}
	}
}

impl ScaleOp<u64> {
	pub fn apply(&self, value: u64) -> u64 {
		match self {
			ScaleOp::Fraction(numerator, denominator) => {
				let n = u64::from(*numerator);
				let d = u64::from(denominator.get());
				value.saturating_mul(n) / d
			}
			ScaleOp::Minus(amount) => value.saturating_sub(*amount),
			ScaleOp::Fixed(amount) => *amount,
			ScaleOp::Zero => 0,
		}
	}
}

impl ScaleOp<usize> {
	pub fn apply(&self, value: Option<usize>) -> Option<usize> {
		match (self, value) {
			(ScaleOp::Fraction(numerator, denominator), Some(value)) => {
				let n = *numerator as usize;
				let d = denominator.get() as usize;
				Some(value.saturating_mul(n) / d)
			}
			(ScaleOp::Minus(amount), Some(value)) => {
				Some(value.saturating_sub(*amount))
			}
			(ScaleOp::Fixed(amount), Some(_)) => Some(*amount),
			(ScaleOp::Zero, _) => Some(0),
			(_, None) => None,
		}
	}
}

impl ScaleOp<Duration> {
	pub fn apply(&self, value: Option<Duration>) -> Option<Duration> {
		match (self, value) {
			(ScaleOp::Fraction(numerator, denominator), Some(value)) => {
				let n = *numerator;
				let d = denominator.get();
				Some(value.saturating_mul(n) / d)
			}
			(ScaleOp::Minus(amount), Some(value)) => {
				Some(value.saturating_sub(*amount))
			}
			(ScaleOp::Fixed(amount), _) => Some(*amount),
			(ScaleOp::Zero, _) => Some(Duration::ZERO),
			(_, None) => None,
		}
	}
}
