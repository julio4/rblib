use {
	crate::{steps::*, *},
	core::time::Duration,
};

#[test]
fn only_steps() {
	let pipeline = Pipeline::<EthereumMainnet>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	println!("{pipeline:#?}");
}

#[test]
fn nested_verbose() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue);

	let nested = Pipeline::<EthereumMainnet>::default()
		.with_step(AppendNewTransactionFromPool)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	let top_level = top_level //
		.with_pipeline(Loop, nested);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise_static() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, AppendNewTransactionFromPool);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise_simulated() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, (RevertProtection,));

	println!("{top_level:#?}");
}

#[test]
fn nested_many_concise() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(
			Loop,
			(
				AppendNewTransactionFromPool,
				PriorityFeeOrdering,
				TotalProfitOrdering,
				RevertProtection,
			),
		);

	println!("{top_level:#?}");
}

#[test]
fn flashblocks_example() {
	struct WebSocketBeginBlock;
	impl Step for WebSocketBeginBlock {
		type Kind = Simulated;

		async fn step<P: Platform>(
			&mut self,
			_payload: SimulatedPayload<P>,
			_ctx: &StepContext<P>,
		) -> ControlFlow<P, Simulated> {
			todo!()
		}
	}

	struct WebSocketEndBlock;
	impl Step for WebSocketEndBlock {
		type Kind = Simulated;

		async fn step<P: Platform>(
			&mut self,
			_payload: SimulatedPayload<P>,
			_ctx: &StepContext<P>,
		) -> ControlFlow<P, Simulated> {
			todo!()
		}
	}

	struct FlashblockEpilogue;
	impl Step for FlashblockEpilogue {
		type Kind = Simulated;

		async fn step<P: Platform>(
			&mut self,
			_payload: SimulatedPayload<P>,
			_ctx: &StepContext<P>,
		) -> ControlFlow<P, Simulated> {
			todo!()
		}
	}

	struct PublishToWebSocket(FlashblocksConfig);
	impl Step for PublishToWebSocket {
		type Kind = Simulated;

		async fn step<P: Platform>(
			&mut self,
			_payload: SimulatedPayload<P>,
			_ctx: &StepContext<P>,
		) -> ControlFlow<P, Simulated> {
			todo!()
		}
	}

	#[derive(Debug)]
	struct FlashblockLimits(FlashblocksConfig);
	impl<P: Platform> LimitsFactory<P> for FlashblockLimits {
		fn create(
			&self,
			_block: &BlockContext<P>,
			_enclosing: Option<&Limits>,
		) -> Limits {
			todo!()
		}
	}

	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	fn make_pipeline<P: Platform>(config: FlashblocksConfig) -> Pipeline<P> {
		Pipeline::<P>::default()
			.with_prologue(OptimismPrologue)
			.with_epilogue(BuilderEpilogue)
			.with_step(WebSocketBeginBlock)
			.with_pipeline(Loop, |nested: Pipeline<P>| {
				nested
					.with_limits(FlashblockLimits(config.clone()))
					.with_epilogue(FlashblockEpilogue)
					.with_pipeline(
						Loop,
						(
							AppendNewTransactionFromPool,
							PriorityFeeOrdering,
							TotalProfitOrdering,
							RevertProtection,
						),
					)
					.with_step(PublishToWebSocket(config))
			})
			.with_step(WebSocketEndBlock)
	}

	let config = FlashblocksConfig {
		count: 5,
		interval: Duration::from_millis(200),
	};

	println!("{:#?}", make_pipeline::<EthereumMainnet>(config));
}
