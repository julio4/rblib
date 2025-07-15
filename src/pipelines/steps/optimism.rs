use {crate::*, reth_node_builder::PayloadBuilderError, std::sync::Arc};

/// This step appends the sequencer transactions that are defined in the payload
/// attributes parameter from the CL node into the payload under construction.
/// It requires that the payload has no existing transactions in it as the
/// sequencer expects its transactions to be at the top of the block.
pub struct OptimismPrologue;
impl Step<Optimism> for OptimismPrologue {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<Optimism>,
		ctx: StepContext<Optimism>,
	) -> ControlFlow<Optimism> {
		let existing_transactions_count = payload.history().transactions().count();
		if existing_transactions_count > 0 {
			return ControlFlow::Fail(PayloadBuilderError::Other(
				format!(
					"Optimism sequencer transactions must be at the top of the block. \
					 This payload already has {existing_transactions_count} \
					 transaction(s).",
				)
				.into(),
			));
		}

		let mut gas_used = 0;

		// Apply all sequencer transactions to the payload.
		let mut payload = payload;
		for tx in &ctx.block().attributes().transactions {
			payload = match payload.apply(tx.clone().into_value()) {
				Ok(payload) => payload,
				Err(e) => {
					return ControlFlow::Fail(PayloadBuilderError::Other(
						format!("Failed to apply sequencer transaction {tx:?}: {e}").into(),
					));
				}
			};

			// ensure that sequencer transactions do not exceed the gas limit.
			// If this happens, fail the whole pipeline because we will never be able
			// to build a valid payload that fits the gas limit.
			gas_used += payload.gas_used();
			if gas_used > ctx.limits().gas_limit {
				return ControlFlow::Fail(PayloadBuilderError::Other(
					format!(
						"Sequencer transactions exceed block gas limit: {gas_used} > {}",
						ctx.limits().gas_limit
					)
					.into(),
				));
			}
		}

		ControlFlow::Ok(payload)
	}
}

#[cfg(test)]
mod tests {
	use {
		crate::{
			steps::OptimismPrologue,
			tests::{OneStep, TransactionRequestExt},
			*,
		},
		std::sync::Arc,
	};

	struct NoOpStep;
	impl Step<Optimism> for NoOpStep {
		async fn step(
			self: Arc<Self>,
			payload: Checkpoint<Optimism>,
			_: StepContext<Optimism>,
		) -> ControlFlow<Optimism> {
			ControlFlow::Ok(payload)
		}
	}

	#[tokio::test]
	async fn sequencer_txs_not_included_without_step() {
		let output = OneStep::new(NoOpStep).run().await.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().transactions().count(), 0);
	}

	#[tokio::test]
	async fn sequencer_txs_are_included() {
		let output = OneStep::new(OptimismPrologue).run().await.unwrap();

		let ControlFlow::Ok(payload) = output else {
			panic!("Expected Ok payload, got: {output:?}");
		};

		assert_eq!(payload.history().transactions().count(), 1);
		assert!(payload.transaction().unwrap().is_deposit());
	}

	#[tokio::test]
	async fn fails_on_non_empty_payload() {
		let output = OneStep::new(OptimismPrologue)
			.with_payload_tx(|tx| tx.transfer().with_default_signer().nonce(0))
			.run()
			.await
			.unwrap();

		let ControlFlow::Fail(err) = output else {
			panic!("Expected Fail, got: {output:?}");
		};

		assert!(err.to_string().contains(
			"Optimism sequencer transactions must be at the top of the block. This \
			 payload already has 1 transaction(s)."
		));
	}
}
