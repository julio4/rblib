use {crate::*, std::sync::Arc};

/// This step eliminates transactions that are reverted from the payload.
pub struct RevertProtection;
impl<P: Platform> Step<P> for RevertProtection {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let history = payload.history();

		if history.is_empty() {
			// if there are no transactions in the payload, we can skip revert
			// protection
			return ControlFlow::Ok(payload);
		}

		// identify the first failed transaction in the payload
		let Some(first_failed) =
			history.iter().find(|checkpoint| !checkpoint.is_success())
		else {
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
			// apply transactions from the unsafe region one by one on top of the
			// safe region in the same order as they appear in the payload.
			let Some(tx) = remaining
				.pop_first()
				.and_then(|tx| tx.transaction().cloned())
			else {
				// if there are no more transactions to process, we can return the
				// safe checkpoint
				break;
			};

			let Ok(new_checkpoint) = safe.apply(tx) else {
				// if the transaction cannot be applied, we skip it and continue
				// with the next one
				continue;
			};

			if new_checkpoint.is_success() {
				// if the transaction was applied successfully, we can
				// use the new checkpoint as the base for the next
				// transaction, otherwise we keep the previous
				// checkpoint as the base because this transaction
				// reverted and we don't want to include it in the payload
				safe = new_checkpoint;
			}
		}

		ControlFlow::Ok(safe)
	}
}
