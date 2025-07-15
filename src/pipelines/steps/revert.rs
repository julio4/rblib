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

		let history = payload.history();

		// First identify a valid prefix of the payload history that does not
		// contain any reverted transactions. This will be the baseline checkpoint
		// that we will use to apply the remaining transactions. Nothing in this
		// prefix needs to be re-executed.

		let Some(prefix_len) = history
			.iter()
			.position(|checkpoint| !checkpoint.is_success())
		else {
			// none of the transactions have reverted, return the payload as is
			return ControlFlow::Ok(payload);
		};

		let (valid, mut remaining) = history.split_at(prefix_len);
		let mut safe = valid.last().cloned().expect("at least baseline checkpoint");

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

#[cfg(test)]
mod tests {
	use {
		super::*,
		crate::pipelines::tests::*,
		alloy::network::TransactionBuilder,
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
