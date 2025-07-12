use {
	crate::*,
	alloy::primitives::Address,
	itertools::Itertools,
	reth::primitives::Recovered,
	reth_ethereum::primitives::SignedTransaction,
	reth_payload_builder::PayloadBuilderError,
	std::{
		collections::{BTreeMap, VecDeque, btree_map::Entry},
		sync::Arc,
	},
};

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

		// group transactions by their sender, and for each sender keep a list of
		// transactions signed by them ordered by their nonce.
		let mut txs = TxsQueue::from(&history);

		// identify the prefix of the payload history where transactions are
		// correctly ordered by effective priority fee, taking into account nonce
		// dependencies of senders.
		let mut prefix_len = 1;

		for (pos, (a, b)) in history.iter().tuple_windows().enumerate() {
			if let (Some(tx_a), Some(tx_b)) = (a.transaction(), b.transaction()) {
				// pop the next eligible transaction for this sender, it should be the
				// current tx_a that we are checking.
				let expected_tx_a =
					txs.pop_by_signer(tx_a.signer()).expect("bug in TxsQueue");

				if expected_tx_a.tx_hash() != tx_a.tx_hash() {
					// the transaction is not in the correct nonce order.
					break;
				}

				// if the transaction is not ordered by effective priority fee,
				if a.effective_tip_per_gas() < b.effective_tip_per_gas() {
					// check if there is a justification for this misordering due to nonce
					// dependencies.

					let best_next_tx = txs
						.peek_best(payload.block().base_fee())
						.expect("should have at least one transaction in the queue");

					if best_next_tx.tx_hash() != tx_b.tx_hash() {
						// the next best transaction in the queue is not the one we are
						// checking, so we can break here, because the order is incorrect.
						break;
					}
				}
			}

			// +2 because we are using tuple_windows which gives us pairs
			// of checkpoints, so we need to add 1 to the position to get the
			// length of the prefix and the first checkpoint is the baseline
			// checkpoint with no tx in it.
			prefix_len = pos + 2;
		}

		if prefix_len == history.len() {
			// the whole history is ordered correctly,
			// we can return the payload as is.
			return ControlFlow::Ok(payload);
		}

		// we need to reorder the payload past the ordered prefix.
		let (ordered, unordered) = history.split_at(prefix_len);

		let mut ordered = ordered
			.last()
			.cloned()
			.expect("at least baseline checkpoint");

		let mut unordered = TxsQueue::from(&unordered);
		while let Some(tx) = unordered.pop_best(payload.block().base_fee()) {
			// apply the next best transaction to the ordered payload.
			ordered = match ordered.apply(tx.clone()) {
				Ok(new_checkpoint) => new_checkpoint,
				Err(e) => return ControlFlow::Fail(PayloadBuilderError::other(e)),
			};
		}

		assert_eq!(
			ordered.depth(),
			history.len(),
			"ordered payload should have the same depth as the original payload"
		);

		ControlFlow::Ok(ordered)
	}
}

/// This type takes a list of transactions and groups them by their signer
/// address, and then sorts them by their nonce within each group. This gives us
/// access to the transactions that are eligible to be included for each signer.
///
/// We're using a btree map here to keep tx order stable across loop iterations,
/// when there are ties in effective priority fee.
struct TxsQueue<'a, P: Platform>(
	BTreeMap<Address, VecDeque<&'a Recovered<types::Transaction<P>>>>,
);

impl<'a, P: Platform> From<&'a Span<P>> for TxsQueue<'a, P> {
	fn from(span: &'a Span<P>) -> Self {
		use alloy::consensus::transaction::Transaction;
		Self(
			span
				.iter()
				.filter_map(|checkpoint| checkpoint.transaction())
				.map(|tx| (tx.signer(), tx))
				.into_group_map()
				.into_iter()
				.map(|(signer, mut txs)| {
					// sort transactions of the same sender by their nonce
					txs.sort_by_key(|tx| tx.nonce());
					(signer, txs.into_iter().collect())
				})
				.collect(),
		)
	}
}

impl<'a, P: Platform> TxsQueue<'a, P> {
	/// Returns an iterator that yields all transactions that are eligible to be
	/// included according to sender nonce dependencies.
	pub fn eligible(
		&self,
	) -> impl Iterator<Item = &'a Recovered<types::Transaction<P>>> {
		self.0.iter().filter_map(|(_, txs)| txs.front()).copied()
	}

	/// Returns the next eligible transaction for a given sender, and removes it
	/// from the queue. Returns `None` if there are no more transactions in the
	/// queue for this sender.
	pub fn pop_by_signer(
		&mut self,
		signer: Address,
	) -> Option<&'a Recovered<types::Transaction<P>>> {
		if let Entry::Occupied(mut txs) = self.0.entry(signer) {
			let tx = txs.get_mut().pop_front().expect("should have been removed");

			if txs.get().is_empty() {
				txs.remove();
			}

			return Some(tx);
		}

		None
	}

	/// Returns the next eligible transaction with the highest effective
	/// priority fee, and removes it from the queue. Returns `None` if there are
	/// no more transactions in the queue.
	pub fn pop_best(
		&mut self,
		base_fee: u64,
	) -> Option<&'a Recovered<types::Transaction<P>>> {
		use alloy::consensus::transaction::Transaction;
		let best_signer = self
			.eligible()
			.max_by_key(|tx| tx.effective_tip_per_gas(base_fee))
			.map(Recovered::signer)?;

		self.pop_by_signer(best_signer)
	}

	/// Returns a reference to the next best eligible transaction without removing
	/// it from the queue.
	pub fn peek_best(
		&self,
		base_fee: u64,
	) -> Option<&'a Recovered<types::Transaction<P>>> {
		use alloy::consensus::transaction::Transaction;
		let best_signer = self
			.eligible()
			.max_by_key(|tx| tx.effective_tip_per_gas(base_fee))
			.map(Recovered::signer)?;

		self
			.0
			.get(&best_signer)
			.and_then(|txs| txs.front())
			.copied()
	}
}
