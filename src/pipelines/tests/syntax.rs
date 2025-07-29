use {
	crate::{prelude::*, steps::*, test_utils::*},
	alloy_origin::signers::local::LocalSigner,
};

#[test]
fn only_steps() {
	let pipeline = Pipeline::<Ethereum>::default()
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()))
		.with_step(GatherBestTransactions)
		.with_step(OrderByPriorityFee)
		.with_step(RemoveRevertedTransactions);

	println!("{pipeline:#?}");
}

#[test]
#[cfg(feature = "optimism")]
fn only_steps_optimism_specific() {
	let pipeline = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_step(GatherBestTransactions)
		.with_step(OrderByPriorityFee)
		.with_step(OrderByTotalProfit)
		.with_step(RemoveRevertedTransactions)
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()));

	println!("{pipeline:#?}");
}

#[test]
fn nested_verbose() {
	let top_level = Pipeline::<Ethereum>::default()
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()));

	let nested = Pipeline::<Ethereum>::default()
		.with_step(AppendOneTransactionFromPool::default())
		.with_step(OrderByPriorityFee)
		.with_step(OrderByTotalProfit)
		.with_step(RemoveRevertedTransactions);

	let top_level = top_level //
		.with_pipeline(Loop, nested);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise() {
	let top_level = Pipeline::<Ethereum>::default()
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()))
		.with_pipeline(Loop, (AppendOneTransactionFromPool::default(),));

	println!("{top_level:#?}");
}

#[test]
#[cfg(feature = "optimism")]
fn nested_many_concise() {
	// synthesize dummy steps
	fake_step!(TestStep1);
	fake_step!(TestStep2);

	let top_level = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()))
		.with_step(TestStep1)
		.with_pipeline(
			Loop,
			(
				AppendOneTransactionFromPool::default(),
				OrderByPriorityFee,
				OrderByTotalProfit,
				RemoveRevertedTransactions,
			),
		)
		.with_step(TestStep2);

	println!("{top_level:#?}");
}

#[test]
#[allow(dead_code)]
#[cfg(feature = "optimism")]
fn flashblocks_example_closure() {
	use core::time::Duration;

	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	fake_step!(WebSocketBeginBlock);
	fake_step!(WebSocketEndBlock);
	fake_step!(FlashblockEpilogue);
	fake_step!(PublishToWebSocket, FlashblocksConfig);

	#[derive(Debug)]
	struct FlashblockLimits(FlashblocksConfig);
	impl<P: Platform> LimitsFactory<P> for FlashblockLimits {
		fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
			unimplemented!()
		}
	}

	let config = FlashblocksConfig {
		count: 5,
		interval: Duration::from_millis(200),
	};

	let pipeline = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()))
		.with_step(WebSocketBeginBlock)
		.with_pipeline(Loop, |nested: Pipeline<Optimism>| {
			nested
				.with_limits(FlashblockLimits(config.clone()))
				.with_epilogue(FlashblockEpilogue)
				.with_pipeline(
					Loop,
					(
						AppendOneTransactionFromPool::default(),
						OrderByPriorityFee,
						OrderByTotalProfit,
						RemoveRevertedTransactions,
					),
				)
				.with_step(PublishToWebSocket(config))
		})
		.with_step(WebSocketEndBlock);

	println!("{pipeline:#?}");
}

#[test]
#[allow(dead_code)]
#[cfg(feature = "optimism")]
fn flashblocks_example_concise() {
	use core::time::Duration;

	#[derive(Debug, Clone)]
	struct FlashblocksConfig {
		count: usize,
		interval: Duration,
	}

	fake_step!(WebSocketBeginBlock);
	fake_step!(WebSocketEndBlock);
	fake_step!(FlashblockEpilogue);
	fake_step!(PublishToWebSocket, FlashblocksConfig);

	#[derive(Debug)]
	struct FlashblockLimits(FlashblocksConfig);
	impl<P: Platform> LimitsFactory<P> for FlashblockLimits {
		fn create(&self, _: &BlockContext<P>, _: Option<&Limits>) -> Limits {
			unimplemented!()
		}
	}

	let config = FlashblocksConfig {
		count: 5,
		interval: Duration::from_millis(200),
	};

	let pipeline = Pipeline::<Optimism>::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue::with_signer(LocalSigner::random()))
		.with_step(WebSocketBeginBlock)
		.with_pipeline(Loop, |nested: Pipeline<Optimism>| {
			nested
				.with_pipeline(
					Loop,
					(
						AppendOneTransactionFromPool::default(),
						OrderByPriorityFee,
						OrderByTotalProfit,
						RemoveRevertedTransactions,
					)
						.with_limits(FlashblockLimits(config.clone()))
						.with_epilogue(FlashblockEpilogue),
				)
				.with_step(PublishToWebSocket(config))
		})
		.with_step(WebSocketEndBlock);

	println!("{pipeline:#?}");
}
