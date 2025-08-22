use {
	super::{FlashblocksPayloadV1, ws::WebSocketPublisher},
	crate::{
		FlashBlocks,
		builders::flashblocks::{
			ExecutionPayloadBaseV1,
			ExecutionPayloadFlashblockDeltaV1,
		},
	},
	core::sync::atomic::{AtomicU64, Ordering},
	parking_lot::RwLock,
	rblib::{
		alloy::{
			consensus::BlockHeader,
			eips::Encodable2718,
			primitives::{B256, Bloom, U256},
		},
		prelude::*,
	},
	reth_node_builder::PayloadBuilderAttributes,
	std::sync::Arc,
};

pub struct PublishFlashblock {
	/// The websocket interface that manages flashblock publishing to external
	/// subscribers.
	sink: Arc<WebSocketPublisher>,

	/// Keeps track of the flashblock number within one payload job.
	block_counter: AtomicU64,

	/// Set once at the begining of the payload job, captures immutable
	/// information about the payload that is being built. This info is derived
	/// from the payload attributes parameter on the FCU from the EL node.
	block_base: RwLock<Option<ExecutionPayloadBaseV1>>,
}

impl PublishFlashblock {
	pub fn to(sink: &Arc<WebSocketPublisher>) -> Self {
		Self {
			sink: Arc::clone(sink),
			block_counter: AtomicU64::new(0),
			block_base: RwLock::new(None),
		}
	}
}

impl Step<FlashBlocks> for PublishFlashblock {
	async fn step(
		self: std::sync::Arc<Self>,
		payload: Checkpoint<FlashBlocks>,
		ctx: StepContext<FlashBlocks>,
	) -> ControlFlow<FlashBlocks> {
		// get a span that convers all payload checkpoints since the last barrier
		// those are the transactions that are going to be in this flashblock.
		// one exception is the first flashblock, we want to get all checkpoints
		// since the begining of the block, because the `OptimismPrologue` step
		// places a barrier after sequencer transactions and we want to broadcast
		// those transactions as well.
		let block_index = self.block_counter.fetch_add(1, Ordering::SeqCst);
		let this_block_span = if block_index == 0 {
			// first block, get all checkpoints
			payload.history()
		} else {
			// subsequent block, get all checkpoints since last barrier
			payload.history_mut()
		};

		if this_block_span.transactions().count() == 0 {
			// nothing to publish, empty flashblocks are not interesting, skip.
			return ControlFlow::Ok(payload);
		}

		let base = self.block_base.read().clone();
		let diff = ExecutionPayloadFlashblockDeltaV1 {
			state_root: B256::ZERO,       // TODO: compute state root
			receipts_root: B256::ZERO,    // TODO: compute receipts root
			logs_bloom: Bloom::default(), // TODO
			gas_used: this_block_span.gas_used(),
			block_hash: B256::ZERO, // TODO: compute block hash
			transactions: this_block_span
				.transactions()
				.map(|tx| tx.encoded_2718().into())
				.collect(),
			withdrawals: vec![],
			withdrawals_root: B256::ZERO, // TODO: compute withdrawals root
		};

		// Push the contents of the payload
		if let Err(e) = self.sink.publish(&FlashblocksPayloadV1 {
			base,
			diff,
			payload_id: ctx.block().payload_id(),
			index: block_index,
			metadata: serde_json::Value::Null,
		}) {
			tracing::error!("Failed to publish flashblock to websocket: {e}");
		}

		// Place a barrier after each published flashblock to freeze the contents
		// of the payload up to this point, since this becomes a publicly committed
		// state.
		let payload = payload.barrier();

		ControlFlow::Ok(payload)
	}

	async fn before_job(
		self: Arc<Self>,
		ctx: StepContext<FlashBlocks>,
	) -> Result<(), PayloadBuilderError> {
		let block_base = ExecutionPayloadBaseV1 {
			parent_beacon_block_root: ctx
				.block()
				.attributes()
				.parent_beacon_block_root()
				.unwrap_or_default(),
			parent_hash: ctx.block().parent().hash(),
			fee_recipient: ctx.block().coinbase(),
			prev_randao: ctx.block().attributes().prev_randao(),
			block_number: ctx.block().number(),
			gas_limit: ctx
				.block()
				.attributes()
				.gas_limit
				.unwrap_or_else(|| ctx.block().parent().header().gas_limit()),
			timestamp: ctx.block().timestamp(),
			extra_data: ctx.block().block_env().extra_data.clone(),
			base_fee_per_gas: U256::from(ctx.block().base_fee()),
		};

		self.block_base.write().replace(block_base);

		Ok(())
	}

	async fn after_job(
		self: Arc<Self>,
		_: StepContext<FlashBlocks>,
		_: Arc<Result<types::BuiltPayload<FlashBlocks>, PayloadBuilderError>>,
	) -> Result<(), PayloadBuilderError> {
		// reset flashblocks block counter
		self.block_counter.store(0, Ordering::SeqCst);
		*self.block_base.write() = None;
		Ok(())
	}
}
