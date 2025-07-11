use {
	crate::{
		Checkpoint,
		ControlFlow,
		EthereumMainnet,
		Pipeline,
		Step,
		StepContext,
		pipelines::{
			exec::ClonablePayloadBuilderError,
			tests::{LocalNode, TransactionBuilder},
		},
		types,
	},
	alloy::eips::Encodable2718,
	reth::primitives::Recovered,
	reth_ethereum::primitives::SignedTransaction,
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
	tokio::sync::{
		Mutex,
		mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
	},
};

/// This test util is used in unit tests for a single step.
///
/// It allows to run a single step with a predefined list of transactions in the
/// payload as an input and returns the output of the step control flow result.
pub struct OneStep {
	pipeline: Pipeline<EthereumMainnet>,
	pool_txs: Vec<Box<dyn FnMut(TransactionBuilder) -> TransactionBuilder>>,
	input_txs: Vec<Box<dyn FnMut(TransactionBuilder) -> TransactionBuilder>>,
	payload_tx: UnboundedSender<Tx>,
	ok_rx: UnboundedReceiver<Checkpoint<EthereumMainnet>>,
	fail_rx: UnboundedReceiver<PayloadBuilderError>,
	break_rx: UnboundedReceiver<Checkpoint<EthereumMainnet>>,
}

type Tx = Recovered<types::Transaction<EthereumMainnet>>;

impl OneStep {
	pub fn new(step: impl Step<EthereumMainnet>) -> Self {
		let (prepopulate, payload_tx) = PopulatePayload::new();
		let (record_ok, ok_rx) = RecordOk::new();
		let (record_fail, fail_rx, break_rx) = RecordBreakAndFail::new();

		let pipeline = Pipeline::default()
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

	/// Adds a new transaction to the input payload of the step.
	///
	/// Note that transactions added through this method will not go through the
	/// mempool and directly into the payload of the step, which means that nonces
	/// need to be set manually because they will not be reported by the mempool
	/// "pending" transactions count
	pub fn with_payload_tx(
		mut self,
		builder: impl FnMut(TransactionBuilder) -> TransactionBuilder + 'static,
	) -> Self {
		self.input_txs.push(Box::new(builder));
		self
	}

	/// Adds a new transaction to the mempool and makes it available to the step.
	pub fn with_pool_tx(
		mut self,
		builder: impl FnMut(TransactionBuilder) -> TransactionBuilder + 'static,
	) -> Self {
		self.pool_txs.push(Box::new(builder));
		self
	}

	pub async fn run(mut self) -> ControlFlow<EthereumMainnet> {
		use alloy::eips::Decodable2718;
		let local_node = LocalNode::ethereum(self.pipeline).await.unwrap();
		let input_txs = self
			.input_txs
			.into_iter()
			.map(|mut tx| tx(local_node.new_transaction()))
			.collect::<Vec<_>>();

		for tx in input_txs {
			let encoded = tx.build().await.encoded_2718();
			let tx =
				types::Transaction::<EthereumMainnet>::decode_2718(&mut &encoded[..])
					.unwrap()
					.try_into_recovered()
					.unwrap();
			self.payload_tx.send(tx).unwrap();
		}

		let pool_txs = self
			.pool_txs
			.into_iter()
			.map(|mut tx| tx(local_node.new_transaction()))
			.collect::<Vec<_>>();

		for tx in pool_txs {
			let _ = tx
				.send()
				.await
				.expect("Failed to send transaction to the pool");
		}

		let _ = local_node.build_new_block().await;

		let ok_res = self.ok_rx.try_recv();
		let break_res = self.break_rx.try_recv();
		let fail_res = self.fail_rx.try_recv();

		if let Ok(ok) = ok_res {
			tracing::info!("Received ok: {ok}");
			return ControlFlow::Ok(ok);
		}

		if let Ok(fail_res) = fail_res {
			tracing::error!("Received fail: {fail_res}");
			return ControlFlow::Fail(fail_res);
		}

		let Ok(break_res) = break_res else {
			unreachable!("did not receive ok, break or fail.")
		};

		tracing::info!("Received break: {break_res}");
		ControlFlow::Break(break_res)
	}
}

struct PopulatePayload {
	receiver: Mutex<UnboundedReceiver<Tx>>,
}

impl PopulatePayload {
	pub fn new() -> (Self, UnboundedSender<Tx>) {
		let (sender, receiver) = unbounded_channel();
		(
			Self {
				receiver: Mutex::new(receiver),
			},
			sender,
		)
	}
}

impl Step<EthereumMainnet> for PopulatePayload {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		let mut payload = payload;
		while let Ok(tx) = self.receiver.lock().await.try_recv() {
			payload = payload.apply(tx).expect("Failed to apply transaction");
		}

		ControlFlow::Ok(payload)
	}
}

struct RecordOk {
	sender: UnboundedSender<Checkpoint<EthereumMainnet>>,
}

impl RecordOk {
	pub fn new() -> (Self, UnboundedReceiver<Checkpoint<EthereumMainnet>>) {
		let (sender, receiver) = unbounded_channel();
		(Self { sender }, receiver)
	}
}

impl Step<EthereumMainnet> for RecordOk {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		tracing::info!("recording ok: {payload}");
		self.sender.send(payload.clone()).unwrap();
		ControlFlow::Ok(payload)
	}
}

struct RecordBreakAndFail {
	fail_sender: UnboundedSender<PayloadBuilderError>,
	break_sender: UnboundedSender<Checkpoint<EthereumMainnet>>,
}

impl RecordBreakAndFail {
	pub fn new() -> (
		Self,
		UnboundedReceiver<PayloadBuilderError>,
		UnboundedReceiver<Checkpoint<EthereumMainnet>>,
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

impl Step<EthereumMainnet> for RecordBreakAndFail {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		tracing::info!("recording break: {payload}");
		self.break_sender.send(payload.clone()).unwrap();
		ControlFlow::Ok(payload)
	}

	async fn after_job(
		self: Arc<Self>,
		result: Arc<
			Result<types::BuiltPayload<EthereumMainnet>, PayloadBuilderError>,
		>,
	) -> Result<(), PayloadBuilderError> {
		if let Err(e) = result.as_ref() {
			tracing::error!("recording fail: {e}");
			self
				.fail_sender
				.send(ClonablePayloadBuilderError::clone_original(e))
				.unwrap();
		}
		Ok(())
	}
}

struct AlwaysBreak;
impl Step<EthereumMainnet> for AlwaysBreak {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		ControlFlow::Break(payload)
	}
}

struct AlwaysOk;
impl Step<EthereumMainnet> for AlwaysOk {
	async fn step(
		self: Arc<Self>,
		payload: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		ControlFlow::Ok(payload)
	}
}

struct AlwaysFail;
impl Step<EthereumMainnet> for AlwaysFail {
	async fn step(
		self: Arc<Self>,
		_: Checkpoint<EthereumMainnet>,
		_: StepContext<EthereumMainnet>,
	) -> ControlFlow<EthereumMainnet> {
		ControlFlow::Fail(PayloadBuilderError::ChannelClosed)
	}
}

#[tokio::test]
async fn break_is_recorded() {
	let step = OneStep::new(AlwaysBreak).run().await;
	assert!(matches!(step, ControlFlow::Break(_)));
}

#[tokio::test]
async fn ok_is_recorded() {
	let step = OneStep::new(AlwaysOk).run().await;
	assert!(matches!(step, ControlFlow::Ok(_)));
}

#[tokio::test]
async fn fail_is_recorded() {
	let step = OneStep::new(AlwaysFail).run().await;
	assert!(matches!(step, ControlFlow::Fail(_)));
}
