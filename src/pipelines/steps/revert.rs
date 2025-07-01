use {crate::*, std::sync::Arc};

/// This step eliminates transactions that are reverted from the payload.
pub struct RevertProtection;
impl<P: Platform> Step<P> for RevertProtection {
	type Kind = Simulated;

	async fn step(
		self: Arc<Self>,
		payload: SimulatedPayload<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		let history = payload.history();

		if history.is_empty() {
			// if there are no transactions in the payload, we can skip revert
			// protection
			return ControlFlow::Ok(payload);
		}

		// identify the first failed transaction in the payload
		let Some(first_failed) = history.iter().find(|checkpoint| {
			!checkpoint
				.result()
				.map(|r| !r.is_success())
				.unwrap_or(false)
		}) else {
			// none of the transactions have reverted, return the payload as is
			return ControlFlow::Ok(payload);
		};

		// This is a contiguous region of transactions from the beginning of the
		// payload that have not reverted. We will use this as a base checkpoint
		// and apply remaining transactions in the payload on top of it. We discard
		// all transactions applied on top of this base that revert.
		let mut safe = first_failed.prev().unwrap_or_else(|| ctx.block().start());

		// We will attempt to apply all those transactions in the same order as they
		// appear in the payload, starting from the first reverted transaction.
		let mut remaining = first_failed
			.to(&payload)
			.expect("we're in a valid linear history");

		loop {
			if let Some(tx) = remaining.head().transaction() {
				safe = match safe.apply(tx.clone()) {
					Ok(new_checkpoint) => {
						if new_checkpoint.result().expect("it is a tx").is_success() {
							// if the transaction was applied successfully, we can
							// use the new checkpoint as the base for the next
							// transaction
							new_checkpoint
						} else {
							// if the transaction reverted, we discard it and keep
							// the previous base checkpoint
							safe
						}
					}
					Err(_) => safe,
				};
			}

			remaining = match remaining.pop_first() {
				Some(remaining) => remaining,
				None => break, // no more transactions to process
			};
		}

		ControlFlow::Ok(safe)
	}
}
