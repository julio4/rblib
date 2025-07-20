use {crate::*, std::sync::Arc};

/// This step eliminates transactions that are reverted from the payload.
pub struct RevertProtection;
impl<P: Platform> Step<P> for RevertProtection {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		if payload.is_empty() {
			// if there are no transactions in the payload, so no reverts
			return ControlFlow::Ok(payload);
		}

		// we're working only with the mutable history of the payload,
		// if a reverting transaction was already commited, we will not remove it.
		let history = payload.history_mut();

		// First identify a valid prefix of the payload history that does not
		// contain any reverted transactions. This will be the baseline checkpoint
		// that we will use to apply the remaining transactions. Nothing in this
		// prefix needs to be re-executed.

		let Some(prefix_len) = history.iter().position(|checkpoint| {
			!checkpoint
				.result()
				.and_then(|r| r.results().first().map(|t| t.is_success()))
				.unwrap_or(true)
		}) else {
			// none of the transactions have reverted, return the payload as is
			return ControlFlow::Ok(payload);
		};

		let (valid, mut remaining) = history.split_at(prefix_len);
		let mut prefix =
			valid.last().cloned().expect("at least baseline checkpoint");

		while let Some(checkpoint) = remaining.pop_first() {
			// If the checkpoint is an individual transaction, then standard revert
			// protection rules apply to it.
			if let Some(tx) = checkpoint.as_transaction().cloned() {
				let Ok(new_checkpoint) = prefix.apply(tx) else {
					// if the transaction cannot be applied because of consensus rules, we
					// skip it and move on to the next one.
					continue;
				};

				let is_success = new_checkpoint
					.result()
					.and_then(|result| result.results().first())
					.is_none_or(|result| result.is_success());

				if is_success {
					// if the transaction was applied and did not revert or halt, we can
					// use the new checkpoint as the base for the next transaction,
					// otherwise we keep the same prefix and discard this checkpoint.
					prefix = new_checkpoint;
					continue;
				}
			}

			if let Some(_bundle) = checkpoint.as_bundle() {
				todo!()
			}
		}

		ControlFlow::Ok(prefix)
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

	#[rblib_test(Ethereum, Optimism)]
	async fn empty_payload<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RevertProtection).run().await.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().len(), 1);
		assert_eq!(payload.history().transactions().count(), 0);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn one_revert_one_ok<P: TestablePlatform>() {
		let output = OneStep::<P>::new(RevertProtection)
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
		let output = OneStep::<P>::new(RevertProtection)
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
		let output = OneStep::<P>::new(RevertProtection)
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
		let output = OneStep::<P>::new(RevertProtection)
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
