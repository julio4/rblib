use {
	rblib::{
		alloy::{
			consensus::transaction::Recovered, primitives::U256,
			signers::local::PrivateKeySigner,
		},
		prelude::*,
		test_utils::{
			BlockContextMocked, FundedAccounts, transfer_tx as test_transfer_tx,
		},
	},
	tracing_subscriber::layer::SubscriberExt,
	tracy_client::*,
};

// RUSTFLAGS="-C force-frame-pointers=yes -C symbol-mangling-version=v0" cargo build --release --example profile-checkpoint-apply

#[global_allocator]
static GLOBAL: ProfiledAllocator<std::alloc::System> =
	ProfiledAllocator::new(std::alloc::System, 100);

const TX: u64 = 1000;

fn main() -> eyre::Result<()> {
	tracing::subscriber::set_global_default(
		tracing_subscriber::registry().with(tracing_tracy::TracyLayer::default()),
	)
	.expect("setup tracy layer");

	let _main = span!("main");
	let block = BlockContext::<Ethereum>::mocked();
	let mut payload = block.start();

	let txs = (0..TX)
		.map(|i| transfer_tx(&FundedAccounts::signer(0), i, U256::from(50_000u64)))
		.collect::<Vec<_>>();

	let span_apply = span!("Checkpoint Building");
	for tx in txs {
		payload = payload.apply(tx)?;
		plot!("Cumulative Gas Used", payload.cumulative_gas_used() as f64);
	}
	drop(span_apply);

	let build_payload = span!("Build Payload");
	payload
		.build_payload()
		.expect("payload should be built successfully");
	drop(build_payload);

	Ok(())
}

fn transfer_tx(
	signer: &PrivateKeySigner,
	nonce: u64,
	value: U256,
) -> Recovered<types::Transaction<Ethereum>> {
	test_transfer_tx::<Ethereum>(signer, nonce, value)
}
