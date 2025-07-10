use {crate::*, std::sync::Arc};

/// This step eliminates transactions that are reverted from the payload.
pub struct RevertProtection;
impl<P: Platform> Step<P> for RevertProtection {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		let history = payload.history();

		if history.is_empty() {
			// if there are no transactions in the payload, we can skip revert
			// protection
			return ControlFlow::Ok(payload);
		}

		// identify the first failed transaction in the payload
		let Some(first_failed) = history
			.iter()
			.position(|checkpoint| !checkpoint.is_success())
		else {
			// none of the transactions have reverted, return the payload as is
			return ControlFlow::Ok(payload);
		};

		// This is a contiguous region of transactions from the beginning of the
		// payload that have not reverted. We will use this as a base checkpoint
		// and apply remaining transactions in the payload on top of it. We discard
		// all transactions applied on top of this base that revert.

		// Split the payload history into two parts:
		// - `safe`: a region of checkpoints that have not reverted, starting from
		//   the beginning of the payload up to the first reverted transaction.
		// - `remaining`: a region of checkpoints where the first reverted
		//   transaction appeared. We will apply all those transactions on top of
		//   the `safe` region in the same order as they appear in the payload.
		let (safe, mut remaining) = history.split_at(first_failed);
		let mut safe = safe.last().cloned().expect("at least baseline checkpoint");

		while let Some(tx) = remaining.pop_first() {
			let tx = tx
				.transaction()
				.cloned()
				.expect("baseline checkpoint is always successful");

			let Ok(new_checkpoint) = safe.apply(tx) else {
				// if the transaction cannot be applied because of consensus rules, we
				// skip it and move on to the next one.
				continue;
			};

			if new_checkpoint.is_success() {
				// if the transaction was applied and did not revert or halt, we can
				// use the new checkpoint as the base for the next transaction,
				// otherwise we keep the same base checkpoint and discard this
				// checkpoint.
				safe = new_checkpoint;
			}
		}

		ControlFlow::Ok(safe)
	}
}
