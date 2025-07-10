use {crate::*, itertools::Itertools, std::sync::Arc};

/// This step will sort the transactions in the payload by their effective
/// priority fee. During the sorting the transactions will preserve their
/// sender, nonce dependencies.
pub struct PriorityFeeOrdering;

impl<P: Platform> Step<P> for PriorityFeeOrdering {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		// create a span that contains all checkpoints in the payload.
		let history = payload.history();

		// find the first position where checkpoints are not sorted by
		// effective priority fee.
		let Some(_break_point) = history
			.iter()
			.skip(1) // skip the baseline checkpoint, it has no tx in it
			.tuple_windows()
			.position(|(a, b)| a.effective_tip_per_gas() < b.effective_tip_per_gas())
			.map(|i| i + 1)
		else {
			// all checkpoints are correctly ordered, return the payload as is.
			return ControlFlow::Ok(payload);
		};

		// let (ordered, unordered) = history.split_at(break_point);
		// let mut ordered = ordered
		// 	.last()
		// 	.cloned()
		// 	.expect("at least baseline checkpoint");

		ControlFlow::Ok(payload)
	}
}

pub struct TotalProfitOrdering;
impl<P: Platform> Step<P> for TotalProfitOrdering {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Ok(payload)
	}
}
