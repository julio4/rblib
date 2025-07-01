use {
	crate::*,
	alloy::consensus::Transaction,
	core::cmp::Reverse,
	itertools::Itertools,
	std::{collections::HashMap, sync::Arc},
};

/// This step will sort the transactions in the payload by their effective
/// priority fee. During the sorting the transactions will preserve their
/// sender, nonce dependencies.
pub struct PriorityFeeOrdering;

impl<P: Platform> Step<P> for PriorityFeeOrdering {
	type Kind = Static;

	async fn step(
		self: Arc<Self>,
		payload: StaticPayload<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P, Static> {
		let txs_count = payload.len();
		let base_fee = ctx.block().base_fee();

		// group all transactions by their sender, we need to maintain nonce
		// dependencies, so we can't just sort them all independently.
		let by_sender = payload.into_iter().chunk_by(|tx| tx.signer());
		let mut by_sender = by_sender
			.into_iter()
			.map(|(sender, txs)| {
				// sort transactions by nonce, so that we can later pop them in order.
				let mut txs = txs.collect::<Vec<_>>();
				txs.sort_by_key(|tx| Reverse(tx.nonce()));
				(sender, txs)
			})
			.collect::<HashMap<_, _>>();

		// now we have all transactions groupped by the sender and sorted by nonce.

		let mut output = Vec::with_capacity(txs_count);

		loop {
			if by_sender.is_empty() {
				// we've processed all transactions
				break;
			}

			// for each group of transactions, identify the sender with the
			// highest effective priority fee for its lowest nonce and pop it from the
			// group.

			let (sender, _) = by_sender
				.iter()
				.max_by_key(|(_, txs)| {
					txs
						.last()
						.expect("we have at least one transaction in the group")
						.effective_tip_per_gas(base_fee)
				})
				.expect("we have at least one group of transactions");
			let sender = *sender;

			// this sender has the highest fee for its lowest nonce tx
			let txs = by_sender
				.get_mut(&sender)
				.expect("we have at least one transaction in the group");

			// remove the tx from this sender's group
			let tx = txs
				.pop()
				.expect("we have at least one transaction in the group");

			if txs.is_empty() {
				// if this sender has no more transactions, remove it from the map
				by_sender.remove(&sender);
			}

			output.push(tx);
		}

		ControlFlow::Ok(output)
	}
}

pub struct TotalProfitOrdering;
impl<P: Platform> Step<P> for TotalProfitOrdering {
	type Kind = Simulated;

	async fn step(
		self: Arc<Self>,
		payload: SimulatedPayload<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P, Simulated> {
		todo!("total profit ordering for {payload:#?}")
	}
}
