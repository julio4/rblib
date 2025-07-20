use {
	crate::{
		alloy::{consensus::Transaction, primitives::Address},
		reth::{ethereum::primitives::SignedTransaction, primitives::Recovered},
		*,
	},
	itertools::Itertools,
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
		// create a span that contains all mutable checkpoints in the payload.
		let history = payload.history_mut();

		// Get the correct order of transactions in the payload
		let ordered = TxsQueue::from(&history).ordered(payload.block().base_fee());

		// Get the current order of transactions in the payload
		let existing = history.transactions();

		// find the position of the first transaction that is not in the correct
		// order
		let Some(first_misordered) = existing
			.zip(ordered.clone())
			.position(|(a, b)| a.tx_hash() != b.tx_hash())
		else {
			// all transactions in the payload are already in the correct order
			return ControlFlow::Ok(payload);
		};

		// we will need to reapply transactions in the correct order skipping the
		// correctly ordered prefix.
		let mut ordered_prefix = history
			.at(first_misordered)
			.cloned()
			.expect("first_misordered is valid index");

		for tx in ordered.skip(first_misordered) {
			ordered_prefix = match ordered_prefix.apply(tx.clone()) {
				Ok(checkpoint) => checkpoint,
				Err(e) => return e.into(),
			}
		}

		assert_eq!(
			ordered_prefix.depth(),
			payload.depth(),
			"payload length should not change after reordering transactions"
		);

		// return the payload with the transactions in the correct order
		ControlFlow::Ok(ordered_prefix)
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
		Self(
			span
				.iter()
				.flat_map(|checkpoint| checkpoint.transactions())
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
		let best_signer = self
			.eligible()
			.max_by_key(|tx| tx.effective_tip_per_gas(base_fee))
			.map(Recovered::signer)?;

		self.pop_by_signer(best_signer)
	}

	/// Returns all transactions in the correct priority fee order respecting
	/// nonce dependencies.
	pub fn ordered(
		mut self,
		base_fee: u64,
	) -> impl Iterator<Item = &'a Recovered<types::Transaction<P>>> + Clone {
		let mut output = Vec::new();

		while let Some(tx) = self.pop_best(base_fee) {
			output.push(tx);
		}

		output.into_iter()
	}
}

#[cfg(test)]
mod tests {
	use {
		super::*,
		crate::{alloy::network::TransactionBuilder, test_utils::*},
	};

	async fn test_ordering<P: TestablePlatform>(
		payload: Vec<(u32, u64, u128)>, // signer_id, nonce, tip
		expected: Vec<(u32, u64, u128)>,
	) {
		let mut step = OneStep::<P>::new(PriorityFeeOrdering);

		for (sender_id, nonce, tip) in payload {
			if sender_id == u32::MAX && nonce == u64::MAX && tip == u128::MAX {
				// this is a barrier
				step = step.with_payload_barrier();
			} else {
				// this is a transaction
				step = step.with_payload_tx(move |b| {
					b.transfer()
						.with_funded_signer(sender_id)
						.with_nonce(nonce)
						.with_max_priority_fee_per_gas(tip)
				});
			}
		}

		let output = step.run().await;
		let Ok(ControlFlow::Ok(payload)) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		let history = payload.history();
		let txs = history.transactions().collect::<Vec<_>>();

		let actual = txs
			.into_iter()
			.map(|tx| {
				(
					FundedAccounts::index(tx.signer()).unwrap(),
					tx.nonce(),
					tx.max_priority_fee_per_gas().unwrap_or_default(),
				)
			})
			.collect::<Vec<_>>();

		assert_eq!(actual, expected);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn empty_payload<P: TestablePlatform>() {
		let payload = vec![];
		test_ordering::<P>(payload.clone(), payload).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn correct_order_remains_unchanged_nonce_deps() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 160),
			(2, 1, 155),
			(1, 1, 150),
			(1, 2, 190),
			(1, 3, 160),
			(2, 2, 120),
			(2, 3, 170),
		];

		test_ordering::<P>(payload.clone(), payload).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn correct_order_remains_unchanged_no_nonce_deps<
		P: TestablePlatform,
	>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 160),
			(3, 0, 155),
			(4, 0, 150),
			(5, 0, 140),
			(6, 0, 130),
			(7, 0, 120),
			(8, 0, 110),
		];

		test_ordering::<P>(payload.clone(), payload).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn everything_unordered_no_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(4, 0, 120),
			(5, 0, 175),
			(6, 0, 190),
			(7, 0, 155),
			(8, 0, 140),
		];

		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(8, 0, 140),
			(4, 0, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn everything_unordered_no_nonce_deps_with_barrier<
		P: TestablePlatform,
	>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(u32::MAX, u64::MAX, u128::MAX), // barrier
			(4, 0, 120),
			(5, 0, 175),
			(6, 0, 190),
			(7, 0, 155),
			(8, 0, 140),
		];

		let expected = vec![
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(6, 0, 190),
			(5, 0, 175),
			(7, 0, 155),
			(8, 0, 140),
			(4, 0, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn everything_unordered_no_nonce_deps_with_two_barriers<
		P: TestablePlatform,
	>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(u32::MAX, u64::MAX, u128::MAX), // barrier
			(4, 0, 120),
			(5, 0, 175),
			(6, 0, 190),
			(u32::MAX, u64::MAX, u128::MAX), // barrier
			(8, 0, 140),
			(7, 0, 155),
		];

		let expected = vec![
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(4, 0, 120),
			(5, 0, 175),
			(6, 0, 190),
			(7, 0, 155),
			(8, 0, 140),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn everything_unordered_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 150),
			(2, 1, 160),
			(1, 1, 120),
			(5, 0, 175),
			(6, 0, 190),
			(7, 0, 155),
			(8, 0, 140),
		];

		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(7, 0, 155),
			(2, 0, 150),
			(2, 1, 160),
			(8, 0, 140),
			(1, 1, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn everything_unordered_nonce_deps_with_barrier<P: TestablePlatform>() {
		let payload = vec![
			(1, 0, 170),
			(2, 0, 150),
			(u32::MAX, u64::MAX, u128::MAX), // barrier
			(2, 1, 160),
			(1, 1, 120),
			(5, 0, 175),
			(6, 0, 190),
			(7, 0, 155),
			(8, 0, 140),
		];

		let expected = vec![
			(1, 0, 170),
			(2, 0, 150),
			(6, 0, 190),
			(5, 0, 175),
			(2, 1, 160),
			(7, 0, 155),
			(8, 0, 140),
			(1, 1, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn partially_unordered_no_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(4, 0, 120),
			(7, 0, 155),
			(8, 0, 140),
		];
		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(8, 0, 140),
			(4, 0, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn partially_unordered_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(2, 0, 150),
			(3, 0, 160),
			(4, 0, 120),
			(4, 1, 155),
			(8, 0, 140),
		];
		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(2, 0, 150),
			(8, 0, 140),
			(4, 0, 120),
			(4, 1, 155),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn only_last_pair_unordered_no_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(4, 0, 120),
			(8, 0, 140),
		];
		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(8, 0, 140),
			(4, 0, 120),
		];

		test_ordering::<P>(payload, expected).await;
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn only_last_pair_unordered_nonce_deps<P: TestablePlatform>() {
		let payload = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(4, 0, 120),
			(4, 1, 140),
		];
		let expected = vec![
			(6, 0, 190),
			(5, 0, 175),
			(1, 0, 170),
			(3, 0, 160),
			(7, 0, 155),
			(2, 0, 150),
			(4, 0, 120),
			(4, 1, 140),
		];

		test_ordering::<P>(payload, expected).await;
	}
}
