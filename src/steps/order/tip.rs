use {
	crate::{
		alloy::primitives::{Address, B256},
		prelude::*,
	},
	std::{
		collections::{BTreeMap, BTreeSet, HashMap, HashSet},
		sync::Arc,
	},
};

/// This step will sort the checkpoints in the payload by their effective
/// priority fee. During the sorting the transactions will preserve their
/// sender, nonce dependencies.
///
/// Sorting happens only for the mutable part of the payload, i.e. after
/// the last barrier checkpoint. Anything prior to the last barrier
/// checkpoint is considered immutable and will not be reordered.
pub struct OrderByPriorityFee;
impl<P: Platform> Step<P> for OrderByPriorityFee {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_ctx: StepContext<P>,
	) -> ControlFlow<P> {
		// create a span that contains all mutable checkpoints in the payload.
		// We're guaranteed that all checkpoints in this span are executables,
		// because the mutable history begins after the last barrier checkpoint.
		let history = payload.history_mut();

		// Find the correct order of orders in the payload.
		let ordered = SortedOrders::from(&history).into_iter();

		// find the position of the first checkpoint that is not in the correct
		// order.
		let Some(first_out_of_order) = history
			.iter()
			.zip(ordered.clone())
			.position(|(a, b)| a.hash() != b.hash())
		else {
			// all checkpoints in the payload are already in the correct order
			return ControlFlow::Ok(payload);
		};

		// we will need to reconstruct the payload in the correct order skipping
		// the correctly ordered prefix.
		let mut ordered_prefix = history
			.at(first_out_of_order)
			.cloned()
			.expect("must be an valid index");

		for item in ordered.skip(first_out_of_order) {
			ordered_prefix = match ordered_prefix.apply(item) {
				Ok(checkpoint) => checkpoint,
				Err(e) => return e.into(),
			};
		}
		assert_eq!(
			ordered_prefix.depth(),
			payload.depth(),
			"payload length should not change after priority fee ordering"
		);

		ControlFlow::Ok(ordered_prefix)
	}
}

struct SortedOrders<'a, P: Platform> {
	/// A map of all signers and the set of orders they have transactions in.
	by_signer: HashMap<Address, HashSet<B256>>,

	/// a map of all orders, sorted by their effective priority fee.
	by_fee: BTreeMap<u128, BTreeSet<B256>>,

	/// A map of all signers the the nonces they have used in sorted order.
	nonces: HashMap<Address, BTreeSet<u64>>,

	/// A map of all checkpoints, keyed by their hash.
	all_orders: HashMap<B256, &'a Checkpoint<P>>,
}

impl<'a, P: Platform> From<&'a Span<P>> for SortedOrders<'a, P> {
	fn from(span: &'a Span<P>) -> Self {
		let executables = span.iter().filter(|checkpoint| !checkpoint.is_barrier());
		let all_orders: HashMap<B256, &'a Checkpoint<P>> = executables
			.map(|checkpoint| (checkpoint.hash().unwrap(), checkpoint))
			.collect();

		let by_signer: HashMap<Address, HashSet<B256>> = all_orders
			.iter()
			.flat_map(|(hash, checkpoint)| {
				checkpoint
					.transactions()
					.iter()
					.map(move |tx| (tx.signer(), *hash))
			})
			.fold(HashMap::new(), |mut acc, (signer, hash)| {
				acc.entry(signer).or_default().insert(hash);
				acc
			});

		let by_fee: BTreeMap<u128, BTreeSet<B256>> = all_orders
			.iter()
			.map(|(hash, checkpoint)| (checkpoint.effective_tip_per_gas(), *hash))
			.fold(BTreeMap::new(), |mut acc, (fee, hash)| {
				acc.entry(fee).or_default().insert(hash);
				acc
			});

		let nonces: HashMap<Address, BTreeSet<u64>> = all_orders
			.values()
			.flat_map(|order| order.nonces())
			.fold(HashMap::new(), |mut acc, (signer, nonce)| {
				acc.entry(signer).or_default().insert(nonce);
				acc
			});

		Self {
			by_signer,
			by_fee,
			nonces,
			all_orders,
		}
	}
}

impl<'a, P: Platform> IntoIterator for SortedOrders<'a, P> {
	type IntoIter = std::vec::IntoIter<Self::Item>;
	type Item = &'a Checkpoint<P>;

	fn into_iter(mut self) -> Self::IntoIter {
		let mut ordered = Vec::with_capacity(self.all_orders.len());

		while let Some(order) = self.pop_best() {
			ordered.push(order);
		}

		ordered.into_iter()
	}
}

impl<'a, P: Platform> SortedOrders<'a, P> {
	pub fn pop_best(&mut self) -> Option<&'a Checkpoint<P>> {
		let mut skip = 0;

		let order = 'order: loop {
			// get the next order with the highest effective priority fee
			let order = *self
				.nth_order_by_fee(skip)
				.and_then(|order| self.all_orders.get(&order))?;

			for (signer, nonce) in order.nonces() {
				// if there is another order for the same signer with a lower nonce,
				// we cannot use this order yet before the lower nonce is included.
				if *self.nonces.get(&signer)?.iter().next()? < nonce {
					// skip this order
					skip += 1;
					continue 'order;
				}
			}

			// this is the best order we can use right now
			break order;
		};

		self.remove_order(order);
		Some(order)
	}

	fn remove_order(&mut self, order: &Checkpoint<P>) {
		// remove nonces entries
		for (signer, nonce) in order.nonces() {
			if let Some(nonces) = self.nonces.get_mut(&signer) {
				nonces.remove(&nonce);
				if nonces.is_empty() {
					self.nonces.remove(&signer);
				}
			}
		}

		let order_hash = order.hash().expect("order is not barrier");
		let fee = order.effective_tip_per_gas();

		// remove by_fee entry
		if let Some(orders) = self.by_fee.get_mut(&fee) {
			orders.remove(&order_hash);
			if orders.is_empty() {
				self.by_fee.remove(&fee);
			}
		}

		// remove by_signer entry
		for signer in order.signers() {
			if let Some(orders) = self.by_signer.get_mut(&signer) {
				orders.remove(&order_hash);
				if orders.is_empty() {
					self.by_signer.remove(&signer);
				}
			}
		}

		// remove from all_orders
		self.all_orders.remove(&order_hash);
	}

	fn nth_order_by_fee(&self, skip: usize) -> Option<B256> {
		let mut skip = skip;

		for orders in self.by_fee.values().rev() {
			if skip < orders.len() {
				let order_hash = orders.iter().nth(skip)?;
				return Some(*order_hash);
			}

			skip = skip.checked_sub(orders.len())?;
		}

		None
	}
}

#[cfg(test)]
mod tests {
	use {
		super::*,
		crate::{
			alloy::{consensus::Transaction, network::TransactionBuilder},
			test_utils::*,
		},
	};

	async fn test_ordering<P: TestablePlatform>(
		payload: Vec<(u32, u64, u128)>, // signer_id, nonce, tip
		expected: Vec<(u32, u64, u128)>,
	) {
		let mut step = OneStep::<P>::new(OrderByPriorityFee);

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
