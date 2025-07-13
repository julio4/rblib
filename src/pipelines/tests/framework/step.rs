use {
	crate::{
		BlockContext,
		Checkpoint,
		ControlFlow,
		Ethereum,
		Limits,
		LimitsFactory,
		Pipeline,
		Platform,
		Step,
		StepContext,
		pipelines::{
			exec::ClonablePayloadBuilderError,
			tests::{
				NetworkSelector,
				TransactionRequestExt,
				framework::{TestNodeFactory, select},
			},
		},
		types,
	},
	alloy::{eips::Encodable2718, network::TransactionBuilder},
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
	tokio::sync::{
		Mutex,
		mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
	},
};

/// This test util is used in unit tests for testing a single step in isolation.
///
/// It allows to run a single step with a predefined list of transactions in the
/// payload as an input and returns the output of the step control flow result.
///
/// The step is invoked with full node facilities and in realistic condition.
pub struct OneStep<P: Platform + NetworkSelector = Ethereum> {
	pipeline: Pipeline<P>,
	pool_txs: Vec<BoxedTxBuilderFn<P>>,
	input_txs: Vec<BoxedTxBuilderFn<P>>,
	payload_tx: UnboundedSender<select::TxEnvelope<P>>,
	ok_rx: UnboundedReceiver<Checkpoint<P>>,
	fail_rx: UnboundedReceiver<PayloadBuilderError>,
	break_rx: UnboundedReceiver<Checkpoint<P>>,
}

impl<P: Platform + NetworkSelector + TestNodeFactory<P>> OneStep<P> {
	pub fn new(step: impl Step<P>) -> Self {
		let (prepopulate, payload_tx) = PopulatePayload::new();
		let (record_ok, ok_rx) = RecordOk::new();
		let (record_fail, fail_rx, break_rx) = RecordBreakAndFail::new();

		let pipeline = Pipeline::<P>::default()
			.with_step(prepopulate)
			.with_step(step)
			.with_step(record_ok)
			.with_epilogue(record_fail);

		Self {
			pipeline,
			input_txs: Vec::new(),
			pool_txs: Vec::new(),
			payload_tx,
			ok_rx,
			fail_rx,
			break_rx,
		}
	}

	pub fn with_limits(mut self, limits: Limits) -> Self {
		struct FixedLimits(Limits);
		impl<P: Platform> LimitsFactory<P> for FixedLimits {
			fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
				self.0.clone()
			}
		}
		self.pipeline = self.pipeline.with_limits(FixedLimits(limits));
		self
	}

	/// Adds a new transaction to the input payload of the step.
	///
	/// Note that transactions added through this method will not go through the
	/// mempool and directly into the payload of the step, which means that nonces
	/// need to be set manually because they will not be reported by the mempool
	/// "pending" transactions count
	pub fn with_payload_tx(
		mut self,
		builder: impl FnMut(
			select::TransactionRequest<P>,
		) -> select::TransactionRequest<P>
		+ 'static,
	) -> Self {
		self.input_txs.push(Box::new(builder));
		self
	}

	/// Adds a new transaction to the mempool and makes it available to the step.
	/// Here we don't need to manage nonces, as the mempool will report the
	/// pending transactions for the signer and nonces will be set automatically.
	pub fn with_pool_tx(
		mut self,
		builder: impl FnMut(
			select::TransactionRequest<P>,
		) -> select::TransactionRequest<P>
		+ 'static,
	) -> Self {
		self.pool_txs.push(Box::new(builder));
		self
	}

	pub async fn run(mut self) -> ControlFlow<P> {
		let local_node = P::create_test_node(self.pipeline).await.unwrap();
		let input_txs = self
			.input_txs
			.into_iter()
			.map(|mut tx| {
				tx(
					local_node
						.build_tx()
						.with_max_fee_per_gas(2_000_000_001)
						.with_max_priority_fee_per_gas(1),
				)
			})
			.collect::<Vec<_>>();

		for tx in input_txs {
			self
				.payload_tx
				.send(tx.build_with_known_signer().expect(
					"Transaction is not using one of the known prefunded signers",
				))
				.unwrap();
		}

		let pool_txs = self
			.pool_txs
			.into_iter()
			.map(|mut tx| tx(local_node.build_tx()))
			.collect::<Vec<_>>();

		for tx in pool_txs {
			let _ = local_node
				.send_tx(tx)
				.await
				.expect("Failed to send transaction to the pool");
		}

		let _ = local_node.next_block().await;

		let ok_res = self.ok_rx.try_recv();
		let break_res = self.break_rx.try_recv();
		let fail_res = self.fail_rx.try_recv();

		if let Ok(ok) = ok_res {
			return ControlFlow::Ok(ok);
		}

		if let Ok(fail_res) = fail_res {
			return ControlFlow::Fail(fail_res);
		}

		let Ok(break_res) = break_res else {
			unreachable!("did not receive ok, break or fail.")
		};

		ControlFlow::Break(break_res)
	}
}

struct PopulatePayload<P: Platform + NetworkSelector> {
	receiver: Mutex<UnboundedReceiver<select::TxEnvelope<P>>>,
}

impl<P: Platform + NetworkSelector> PopulatePayload<P> {
	pub fn new() -> (Self, UnboundedSender<select::TxEnvelope<P>>) {
		let (sender, receiver) = unbounded_channel();
		(
			Self {
				receiver: Mutex::new(receiver),
			},
			sender,
		)
	}
}

impl<P: Platform + NetworkSelector> Step<P> for PopulatePayload<P> {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		use alloy::eips::Decodable2718;

		let mut payload = payload;
		while let Ok(tx) = self.receiver.lock().await.try_recv() {
			// unfortunatelly encoding and dcoding with 2718 is the only generic way
			// to handle transaction types across different platforms and networks
			// and SDKs that we are working with.
			let encoded = tx.encoded_2718();
			let decoded = types::Transaction::<P>::decode_2718(&mut &encoded[..])
				.expect("Failed to decode transaction");
			payload = payload.apply(decoded).expect("Failed to apply transaction");
		}

		ControlFlow::Ok(payload)
	}
}

struct RecordOk<P: Platform> {
	sender: UnboundedSender<Checkpoint<P>>,
}

impl<P: Platform> RecordOk<P> {
	pub fn new() -> (Self, UnboundedReceiver<Checkpoint<P>>) {
		let (sender, receiver) = unbounded_channel();
		(Self { sender }, receiver)
	}
}

impl<P: Platform> Step<P> for RecordOk<P> {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		self.sender.send(payload.clone()).unwrap();
		ControlFlow::Ok(payload)
	}
}

struct RecordBreakAndFail<P: Platform> {
	fail_sender: UnboundedSender<PayloadBuilderError>,
	break_sender: UnboundedSender<Checkpoint<P>>,
}

impl<P: Platform> RecordBreakAndFail<P> {
	pub fn new() -> (
		Self,
		UnboundedReceiver<PayloadBuilderError>,
		UnboundedReceiver<Checkpoint<P>>,
	) {
		let (fail_sender, fail_receiver) = unbounded_channel();
		let (break_sender, break_receiver) = unbounded_channel();
		(
			Self {
				fail_sender,
				break_sender,
			},
			fail_receiver,
			break_receiver,
		)
	}
}

impl<P: Platform> Step<P> for RecordBreakAndFail<P> {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		self.break_sender.send(payload.clone()).unwrap();
		ControlFlow::Ok(payload)
	}

	async fn after_job(
		self: Arc<Self>,
		result: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
	) -> Result<(), PayloadBuilderError> {
		if let Err(e) = result.as_ref() {
			self
				.fail_sender
				.send(ClonablePayloadBuilderError::from(e).into())
				.unwrap();
		}
		Ok(())
	}
}

struct AlwaysBreak;
impl<P: Platform> Step<P> for AlwaysBreak {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Break(payload)
	}
}

struct AlwaysOk;
impl<P: Platform> Step<P> for AlwaysOk {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Ok(payload)
	}
}

struct AlwaysFail;
impl<P: Platform> Step<P> for AlwaysFail {
	async fn step(
		self: Arc<Self>,
		_: Checkpoint<P>,
		_: StepContext<P>,
	) -> ControlFlow<P> {
		ControlFlow::Fail(PayloadBuilderError::ChannelClosed)
	}
}

#[tokio::test]
async fn break_is_recorded() {
	let step = OneStep::<Ethereum>::new(AlwaysBreak).run().await;
	assert!(matches!(step, ControlFlow::Break(_)));
}

#[tokio::test]
async fn ok_is_recorded() {
	let step = OneStep::<Ethereum>::new(AlwaysOk).run().await;
	assert!(matches!(step, ControlFlow::Ok(_)));
}

#[tokio::test]
async fn fail_is_recorded() {
	let step = OneStep::<Ethereum>::new(AlwaysFail).run().await;
	assert!(matches!(step, ControlFlow::Fail(_)));
}

type BoxedTxBuilderFn<P> = Box<
	dyn FnMut(select::TransactionRequest<P>) -> select::TransactionRequest<P>,
>;
