use {
	crate::{steps::*, *},
	core::time::Duration,
};

#[test]
fn only_steps() {
	let pipeline = Pipeline::<EthereumMainnet>::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(RevertProtection);

	println!("{pipeline:#?}");
}

#[test]
fn only_steps_optimism_specific() {
	let pipeline = Pipeline::<Optimism>::default()
		.with_epilogue(BuilderEpilogue)
		.with_prologue(OptimismPrologue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	println!("{pipeline:#?}");
}

#[test]
fn nested_verbose() {
	let top_level =
		Pipeline::<EthereumMainnet>::default().with_epilogue(BuilderEpilogue);

	let nested = Pipeline::<EthereumMainnet>::default()
		.with_step(AppendOneTransactionFromPool::default())
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	let top_level = top_level //
		.with_pipeline(Loop, nested);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, (AppendOneTransactionFromPool::default(),));

	println!("{top_level:#?}");
}

#[test]
fn nested_many_concise() {
	// synthesize dummy steps
	make_step!(TestStep1);
	make_step!(TestStep2);

	let top_level = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_step(TestStep1)
		.with_pipeline(
			Loop,
			(
				AppendOneTransactionFromPool::default(),
				PriorityFeeOrdering,
				TotalProfitOrdering,
				RevertProtection,
			),
		)
		.with_step(TestStep2);

	println!("{top_level:#?}");
}

#[test]
#[allow(dead_code)]
fn flashblocks_example_closure() {
	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	make_step!(WebSocketBeginBlock);
	make_step!(WebSocketEndBlock);
	make_step!(FlashblockEpilogue);
	make_step!(PublishToWebSocket, FlashblocksConfig);

	#[derive(Debug)]
	struct FlashblockLimits(FlashblocksConfig);
	impl<P: Platform> LimitsFactory<P> for FlashblockLimits {
		fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
			todo!()
		}
	}

	let config = FlashblocksConfig {
		count: 5,
		interval: Duration::from_millis(200),
	};

	let pipeline = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_step(WebSocketBeginBlock)
		.with_pipeline(Loop, |nested: Pipeline<Optimism>| {
			nested
				.with_limits(FlashblockLimits(config.clone()))
				.with_epilogue(FlashblockEpilogue)
				.with_pipeline(
					Loop,
					(
						AppendOneTransactionFromPool::default(),
						PriorityFeeOrdering,
						TotalProfitOrdering,
						RevertProtection,
					),
				)
				.with_step(PublishToWebSocket(config))
		})
		.with_step(WebSocketEndBlock);

	println!("{pipeline:#?}");
}

#[test]
#[allow(dead_code)]
fn flashblocks_example_concise() {
	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	make_step!(WebSocketBeginBlock);
	make_step!(WebSocketEndBlock);
	make_step!(FlashblockEpilogue);
	make_step!(PublishToWebSocket, FlashblocksConfig);

	#[derive(Debug)]
	struct FlashblockLimits(FlashblocksConfig);
	impl<P: Platform> LimitsFactory<P> for FlashblockLimits {
		fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
			todo!()
		}
	}

	let config = FlashblocksConfig {
		count: 5,
		interval: Duration::from_millis(200),
	};

	let pipeline = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_step(WebSocketBeginBlock)
		.with_pipeline(Loop, |nested: Pipeline<Optimism>| {
			nested
				.with_pipeline(
					Loop,
					(
						AppendOneTransactionFromPool::default(),
						PriorityFeeOrdering,
						TotalProfitOrdering,
						RevertProtection,
					)
						.with_limits(FlashblockLimits(config.clone()))
						.with_epilogue(FlashblockEpilogue),
				)
				.with_step(PublishToWebSocket(config))
		})
		.with_step(WebSocketEndBlock);

	println!("{pipeline:#?}");
}
