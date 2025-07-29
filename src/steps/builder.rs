use {
	crate::{alloy, prelude::*, reth},
	alloy::{
		consensus::SignableTransaction,
		eips::eip7623::TOTAL_COST_FLOOR_PER_TOKEN,
		network::TransactionBuilder,
		primitives::{Address, Bytes},
		signers::local::PrivateKeySigner,
	},
	op_alloy::network::TxSignerSync,
	reth::ethereum::chainspec::EthChainSpec,
	std::sync::Arc,
	tracing::warn,
};

pub struct BuilderEpilogue<P: PlatformWithRpcTypes> {
	signer: PrivateKeySigner,
	required: bool,
	message_fn: MessageFn<P>,
}

impl<P: PlatformWithRpcTypes> BuilderEpilogue<P> {
	pub fn with_signer(signer: PrivateKeySigner) -> Self {
		Self {
			signer,
			required: false,
			message_fn: Arc::new(|block: &BlockContext<P>| {
				format!("flashbots rblib block #{}", block.number())
			}) as MessageFn<P>,
		}
	}

	/// Specifies custom logic to generate the message for the epilogue
	/// transaction.
	#[must_use]
	pub fn with_message<F>(mut self, message_fn: F) -> Self
	where
		F: Fn(&BlockContext<P>) -> String + Send + Sync + 'static,
	{
		self.message_fn = Arc::new(message_fn);
		self
	}

	/// If the builder epilogue is required then the payload will fail if the
	/// transaction is not included, otherwise the epilogue transaction will be
	/// treated as a nice to have optional.
	#[must_use]
	pub fn required(mut self) -> Self {
		self.required = true;
		self
	}

	/// Creates an instance of `LimitsFactory` that will account for the epilogue
	/// transaction when calculating the limits for the block and inherit all
	/// limits from either the enclosing limits or the default limits for the
	/// platform.
	pub fn limiter(&self) -> impl LimitsFactory<P> {
		LimitsMinusEpilogue {
			message_fn: Arc::clone(&self.message_fn),
		}
	}
}

impl<P: PlatformWithRpcTypes> Step<P> for BuilderEpilogue<P> {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let message: Bytes = (self.message_fn)(ctx.block()).into_bytes().into();
		let gas_estimate = estimate_gas_for_tx(&message);
		let signer_nonce = match payload.nonce_of(self.signer.address()) {
			Ok(nonce) => nonce,
			Err(e) => {
				return ControlFlow::Fail(PayloadBuilderError::other(e));
			}
		};

		let builder_tx = types::TransactionRequest::<P>::default()
			.with_chain_id(ctx.block().chainspec().chain_id())
			.with_nonce(signer_nonce)
			.with_gas_limit(gas_estimate)
			.with_max_fee_per_gas(ctx.block().base_fee().into())
			.with_max_priority_fee_per_gas(0)
			.with_to(Address::ZERO)
			.with_input(message)
			.build_unsigned();

		let mut builder_tx = match builder_tx {
			Ok(tx) => tx,
			Err(error) => {
				if self.required {
					return ControlFlow::Fail(PayloadBuilderError::Other(error.into()));
				}

				warn!(
					"Failed to build builder epilogue transaction, but it is not marked \
					 as required, skipping: {error:?}"
				);

				return ControlFlow::Ok(payload);
			}
		};

		let signature = match self.signer.sign_transaction_sync(&mut builder_tx) {
			Ok(signature) => signature,
			Err(error) => {
				if self.required {
					return ControlFlow::Fail(PayloadBuilderError::Other(error.into()));
				}

				warn!(
					"Failed to sign builder epilogue transaction, but it is not marked \
					 as required, skipping: {error:?}"
				);

				return ControlFlow::Ok(payload);
			}
		};

		let signed_tx: types::TxEnvelope<P> =
			builder_tx.into_signed(signature).into();
		let signed_tx: types::Transaction<P> = signed_tx.into();

		let new_checkpoint = match payload.apply(signed_tx) {
			Ok(checkpoint) => checkpoint,
			Err(error) => {
				if self.required {
					return ControlFlow::Fail(PayloadBuilderError::Other(error.into()));
				}

				warn!(
					"Failed to append builder transaction to payload, but it is not \
					 marked as required, skipping: {error:?}"
				);

				return ControlFlow::Ok(payload);
			}
		};

		ControlFlow::Ok(new_checkpoint)
	}
}

type MessageFn<P: Platform> =
	Arc<dyn Fn(&BlockContext<P>) -> String + Send + Sync + 'static>;

struct LimitsMinusEpilogue<P: Platform> {
	message_fn: MessageFn<P>,
}

impl<P: Platform> LimitsFactory<P> for LimitsMinusEpilogue<P> {
	fn create(
		&self,
		block: &BlockContext<P>,
		enclosing: Option<&Limits>,
	) -> Limits {
		// Identify the baseline gas budget we have to work with.
		let mut baseline = enclosing
			.cloned()
			.unwrap_or_else(|| P::DefaultLimits::default().create(block, None));

		// calculate the gas usage for the epilogue transaction
		let message = (self.message_fn)(block);
		let gas_estimate = estimate_gas_for_tx(message.as_bytes());

		// Subtract the gas estimate for the epilogue transaction from the
		// baseline gas limit.
		baseline.gas_limit = baseline.gas_limit.saturating_sub(gas_estimate);

		if let Some(ref mut max_tx_count) = baseline.max_transactions {
			// If we have a max transaction count, we need to account for the
			// epilogue transaction.
			*max_tx_count = max_tx_count.saturating_sub(1);
		}

		baseline
	}
}

fn estimate_gas_for_tx(message: &[u8]) -> u64 {
	// Count zero and non-zero bytes
	let (zero_bytes, nonzero_bytes) =
		message.iter().fold((0, 0), |(zeros, nonzeros), &byte| {
			if byte == 0 {
				(zeros + 1, nonzeros)
			} else {
				(zeros, nonzeros + 1)
			}
		});

	// Calculate gas cost (4 gas per zero byte, 16 gas per non-zero byte)
	let zero_cost = zero_bytes * 4;
	let nonzero_cost = nonzero_bytes * 16;

	// Tx gas should be not less than floor gas https://eips.ethereum.org/EIPS/eip-7623
	let tokens_in_calldata = zero_bytes + nonzero_bytes * 4;
	let floor_gas = 21_000 + tokens_in_calldata * TOTAL_COST_FLOOR_PER_TOKEN;

	std::cmp::max(zero_cost + nonzero_cost + 21_000, floor_gas)
}
