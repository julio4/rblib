use {
	crate::{steps::*, *},
	core::time::Duration,
	std::sync::Arc,
};

#[test]
fn only_steps() {
	let pipeline = Pipeline::<EthereumMainnet>::default()
		.with_epilogue(BuilderEpilogue)
		.with_step(GatherBestTransactions)
		.with_step(PriorityFeeOrdering)
		.with_step(TotalProfitOrdering)
		.with_step(RevertProtection);

	println!("{pipeline:#?}");
}

#[test]
fn only_steps_optimism_specific() {
	let pipeline = Pipeline::<Optimism>::default()
		.with_epilogue(BuilderEpilogue)
		.with_epilogue(OptimismPrologue)
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
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, AppendNewTransactionFromPool);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise_simulated() {
	let top_level = Pipeline::<EthereumMainnet>::default()
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, (AppendNewTransactionFromPool,));

	println!("{top_level:#?}");
}

#[test]
fn nested_many_concise() {
	let top_level = Pipeline::<Optimism>::default()
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
	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	make_step!(WebSocketBeginBlock, Simulated);
	make_step!(WebSocketEndBlock, Simulated);
	make_step!(FlashblockEpilogue, Static);
	make_step!(PublishToWebSocket, Simulated, FlashblocksConfig);

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
						AppendNewTransactionFromPool,
						PriorityFeeOrdering,
						TotalProfitOrdering,
						RevertProtection,
					),
				)
				.with_step(PublishToWebSocket(config))
		})
		.with_step(WebSocketEndBlock);

	println!("{:#?}", pipeline);
}
