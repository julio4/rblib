use {crate::*, std::sync::Arc};

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
		// let history = payload.history();

		// identify the first checkpoint that is not correctly ordered by the
		// priority fee.

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
		todo!("total profit ordering for {payload:#?}")
	}
}
