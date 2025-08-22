use {
	crate::{args::BuilderArgs, platform::FlashBlocks},
	rblib::{pool::OrderPool, prelude::*, steps::*},
	std::sync::Arc,
	step::PublishFlashblock,
	ws::WebSocketPublisher,
};

mod primitives;
mod step;
mod ws;

pub use primitives::*;

pub fn pipeline(
	cli_args: &BuilderArgs,
	pool: &OrderPool<FlashBlocks>,
) -> eyre::Result<Pipeline<FlashBlocks>> {
	let socket_address = cli_args
		.flashblocks_args
		.ws_address()
		.expect("WebSocket address must be set for Flashblocks");

	let interval = cli_args.flashblocks_args.interval;
	let sink = Arc::new(WebSocketPublisher::new(socket_address)?);

	let pipeline = Pipeline::<FlashBlocks>::named("flashblocks")
		.with_prologue(OptimismPrologue)
		.with_pipeline(
			Loop,
			Pipeline::default()
				.with_pipeline(
					Loop,
					(
						AppendOrders::from_pool(pool).with_ok_on_limit(),
						OrderByPriorityFee::default(),
						RemoveRevertedTransactions::default(),
						BreakAfterDeadline,
					)
						.with_epilogue(PublishFlashblock::to(&sink))
						.with_limits(Scaled::default().deadline(Fixed(interval))),
				)
				.with_step(BreakAfterDeadline),
		);

	sink.watch_shutdown(&pipeline);

	Ok(pipeline)
}
