use {
	crate::{alloy, prelude::*, reth},
	alloy::primitives::TxHash,
	derive_more::Deref,
	reth::{ethereum::primitives::SignedTransaction, primitives::Recovered},
	std::sync::Arc,
};

/// This step removes transactions that are reverted from the payload.
///
/// For loose transactions, this means that if the transaction is reverted, it
/// will be removed from the payload.
///
/// For bundles, reverted transactions that are also marked as optional (see
/// [`Bundle::is_optional`]) will be removed from the payload.
pub struct RemoveRevertedTransactions;
impl<P: Platform> Step<P> for RemoveRevertedTransactions {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		if payload.is_empty() {
			// if there are no transactions in the payload, so no reverts
			return ControlFlow::Ok(payload);
		}

		// we're working only with the mutable history of the payload,
		// if a reverting transaction was already commited, we will not remove it.
		let history = payload.history_mut();

		// First identify a valid prefix of the payload history that does not
		// contain any reverts we can remove. This will be the baseline checkpoint
		// that we will use to apply the remaining transactions. Nothing in this
		// prefix needs to be re-executed.
		let Some(prefix_len) = history.iter().position(has_failures) else {
			// none of the transactions have reverted, return the payload as is
			return ControlFlow::Ok(payload);
		};

		let (valid, mut remaining) = history.split_at(prefix_len);
		let mut prefix =
			valid.last().cloned().expect("at least baseline checkpoint");

		// Walk through the checkpoints after the safe prefix and try extending the
		// safe prefix with them. If extending the prefix with a given checkpoint
		// fails, it will be dropped from the payload.
		'next_order: while let Some(checkpoint) = remaining.pop_first() {
			// If the checkpoint is an individual transaction, then standard revert
			// protection rules apply to it.
			if let Some(tx) = checkpoint.as_transaction().cloned() {
				let Ok(new_checkpoint) = prefix.apply(tx.clone()) else {
					// if the transaction cannot be applied because of consensus rules, we
					// skip it emit an event and move on to the next one.
					ctx.emit(TransactionDropped::<P>(tx));
					continue 'next_order;
				};

				if has_failures(&new_checkpoint) {
					// if the transaction did not have a successful outcome, we omit it
					// from the payload by keeping the safe prefix unmodified and emit an
					// event about it.
					ctx.emit(TransactionDropped::<P>(tx));
				} else {
					// if the transaction was applied and had successful outcome, we can
					// extend the safe prefix with it and try the next checkpoint in the
					// remaining list.
					prefix = new_checkpoint;
				}

				continue 'next_order;
			}

			// If the checkpoint is a bundle, we need to check if we can remove any of
			// its optional transactions that have reverted.
			if let Some(mut bundle) = checkpoint.as_bundle().cloned() {
				loop {
					let Ok(new_checkpoint) = prefix.apply(bundle.clone()) else {
						// Bundle cannot be applied anymore. It became invalid due to some
						// previously removed transactions.
						ctx.emit(BundleDropped::<P>(bundle));
						continue 'next_order;
					};

					// the bundle created a valid checkpoint, check if there are any
					// optional transactions that reverted and can be removed while
					// maintaining the bundle's validity.

					let optional_failed_txs = new_checkpoint
						.failed_txs()
						.filter_map(|tx| {
							bundle.is_optional(*tx.tx_hash()).then_some(*tx.tx_hash())
						})
						.collect::<Vec<_>>();

					if optional_failed_txs.is_empty() {
						// nothing to remove, extend the prefix with the new checkpoint
						prefix = new_checkpoint;
						continue 'next_order;
					}

					ctx.emit(BundlePartiallyDropped::<P> {
						bundle: bundle.clone(),
						removed: optional_failed_txs.clone(),
					});

					// try with a version of the bundle that has all optional reverting
					// transactions removed
					for tx in optional_failed_txs {
						bundle = bundle.without_transaction(tx);
					}
				}
			}
		}

		ControlFlow::Ok(prefix)
	}
}

// This is the logic for checking if a checkpoint has failed transactions
// that we can remove. This takes into account both individual
// transactions and bundles.
fn has_failures<P: Platform>(checkpoint: &Checkpoint<P>) -> bool {
	let Some(result) = checkpoint.result() else {
		return false;
	};

	// checkpoint is an individual transaction, just check the outcome.
	if result.source().is_transaction() {
		return result.results().iter().any(|t| !t.is_success());
	} else if let Executable::Bundle(bundle) = result.source() {
		// checkpoint is a bundle, we are looking for transactions
		// in the bundle that are don't have successful outcome and the bundle
		// marks them as optional.
		return result
			.results()
			.iter()
			.map(|r| !r.is_success())
			.zip(result.transactions().iter().map(|tx| tx.tx_hash()))
			.any(|(is_failure, tx_hash)| is_failure && bundle.is_optional(*tx_hash));
	}

	false
}

/// Event emitted by this step when a transaction is dropped from the payload
/// because it had a non-successful execution outcome.
#[derive(Clone, Deref)]
pub struct TransactionDropped<P: Platform>(
	pub Recovered<types::Transaction<P>>,
);

/// Event emitted by this step when a bundle in its entirety is dropped
/// from the payload.
#[derive(Clone, Deref)]
pub struct BundleDropped<P: Platform>(pub types::Bundle<P>);

/// Event emitted by this step when a bundle has some of its optional
/// transactions dropped from the payload because they had a non-successful
/// execution outcome.
#[derive(Clone)]
pub struct BundlePartiallyDropped<P: Platform> {
	pub bundle: types::Bundle<P>,
	pub removed: Vec<TxHash>,
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

	#[rblib_test(Ethereum, Optimism)]
	async fn empty_payload<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RemoveRevertedTransactions)
			.run()
			.await
			.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().len(), 1);
		assert_eq!(payload.history().transactions().count(), 0);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn one_revert_one_ok<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RemoveRevertedTransactions)
			.with_payload_tx(|tx| tx.transfer().with_default_signer().with_nonce(0))
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(1))
			.run()
			.await
			.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().transactions().count(), 1);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn all_revert<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RemoveRevertedTransactions)
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(0))
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(1))
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(2))
			.run()
			.await
			.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().len(), 1);
		assert_eq!(payload.history().transactions().count(), 0);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn all_revert_with_barrier<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RemoveRevertedTransactions)
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(0))
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(1))
			.with_payload_barrier()
			.with_payload_tx(|tx| tx.reverting().with_default_signer().with_nonce(2))
			.run()
			.await
			.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		// all transactions prior to the barrier should not be modified.
		assert_eq!(payload.history().len(), 4); // baseline + 2 tx + barrier
		assert_eq!(payload.history().transactions().count(), 2);

		let history = payload.history();
		let txs = history.transactions().collect::<Vec<_>>();
		assert_eq!(txs[0].nonce(), 0);
		assert_eq!(txs[1].nonce(), 1);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn none_revert<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RemoveRevertedTransactions)
			.with_payload_tx(|tx| tx.transfer().with_default_signer().with_nonce(0))
			.with_payload_tx(|tx| tx.transfer().with_default_signer().with_nonce(1))
			.with_payload_tx(|tx| tx.transfer().with_default_signer().with_nonce(2))
			.run()
			.await
			.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().len(), 4);
		assert_eq!(payload.history().transactions().count(), 3);
	}
}
