use {super::steps::*, crate::*};

#[test]
fn only_steps() {
	let pipeline = Pipeline::default()
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
	let top_level = Pipeline::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue);

	let nested = Pipeline::default()
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
	let top_level = Pipeline::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, AppendNewTransactionFromPool);

	println!("{top_level:#?}");
}

#[test]
fn nested_one_concise_simulated() {
	let top_level = Pipeline::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_pipeline(Loop, (RevertProtection,));

	println!("{top_level:#?}");
}

#[test]
fn nested_many_concise() {
	let top_level = Pipeline::default()
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

		async fn step(
			&mut self,
			_payload: SimulatedPayload,
			_ctx: &mut SimulatedContext,
		) -> ControlFlow<Simulated> {
			todo!()
		}
	}

	struct WebSocketEndBlock;
	impl Step for WebSocketEndBlock {
		type Kind = Simulated;

		async fn step(
			&mut self,
			_payload: SimulatedPayload,
			_ctx: &mut SimulatedContext,
		) -> ControlFlow<Simulated> {
			todo!()
		}
	}

	struct FlashblockEpilogue;
	impl Step for FlashblockEpilogue {
		type Kind = Simulated;

		async fn step(
			&mut self,
			_payload: SimulatedPayload,
			_ctx: &mut SimulatedContext,
		) -> ControlFlow<Simulated> {
			todo!()
		}
	}

	struct PublishToWebSocket;
	impl Step for PublishToWebSocket {
		type Kind = Simulated;

		async fn step(
			&mut self,
			_payload: SimulatedPayload,
			_ctx: &mut SimulatedContext,
		) -> ControlFlow<Simulated> {
			todo!()
		}
	}

	#[derive(Debug)]
	struct FlashblockLimits;
	impl Limits for FlashblockLimits {}

	let fb = Pipeline::default()
		.with_prologue(OptimismPrologue)
		.with_epilogue(BuilderEpilogue)
		.with_step(WebSocketBeginBlock)
		.with_pipeline(Loop, |nested: Pipeline| {
			nested
				.with_limits(FlashblockLimits)
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
				.with_step(PublishToWebSocket)
		})
		.with_step(WebSocketEndBlock);

	println!("{fb:#?}");
}
