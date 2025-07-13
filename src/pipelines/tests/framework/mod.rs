mod accounts;
mod driver;
mod node;
mod step;
mod utils;

pub use {accounts::FundedAccounts, step::OneStep, utils::*};
use {
	alloy::{
		consensus::{SignableTransaction, Signed},
		signers::Signature,
	},
	reth_ethereum::primitives::SignedTransaction,
};

pub const ONE_ETH: u128 = 1_000_000_000_000_000_000;
pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 30_000_000;

/// A type used in tests to bind alloy's network traits to a specific platform.
pub trait NetworkSelector {
	type Network: alloy::network::Network<
			UnsignedTx: SignableTransaction<Signature>,
			TxEnvelope: From<Signed<select::UnsignedTx<Self>, Signature>>
			              + SignedTransaction,
		>;
}

#[cfg(feature = "ethereum")]
mod ethereum;

#[cfg(feature = "optimism")]
mod optimism;

#[cfg(feature = "ethereum")]
impl NetworkSelector for crate::Ethereum {
	type Network = alloy::network::Ethereum;
}

#[cfg(feature = "optimism")]
impl NetworkSelector for crate::Optimism {
	type Network = op_alloy::network::Optimism;
}

/// This trait is used to automatically select the correct local test node type
/// based on the platform that is being tested.
pub trait TestNodeFactory<P: crate::Platform + NetworkSelector> {
	type ConsensusDriver: node::ConsensusDriver<P>;

	async fn create_test_node(
		pipeline: crate::Pipeline<P>,
	) -> eyre::Result<node::LocalNode<P, Self::ConsensusDriver>>;
}

mod select {
	use super::NetworkSelector;

	pub type Network<P: NetworkSelector> = <P as NetworkSelector>::Network;

	pub type BlockResponse<P: NetworkSelector> =
		<Network<P> as alloy::network::Network>::BlockResponse;

	pub type TxEnvelope<P: NetworkSelector> =
		<Network<P> as alloy::network::Network>::TxEnvelope;

	pub type UnsignedTx<P: NetworkSelector> =
		<Network<P> as alloy::network::Network>::UnsignedTx;

	pub type TransactionRequest<P: NetworkSelector> =
		<Network<P> as alloy::network::Network>::TransactionRequest;
}

/// This gets invoked before any tests, when the cargo test framework loads the
/// test library.
#[cfg(test)]
#[ctor::ctor]
fn init_tests() {
	use tracing_subscriber::{filter::filter_fn, prelude::*};
	if let Ok(v) = std::env::var("TEST_TRACE") {
		#[allow(clippy::match_same_arms)]
		let level = match v.as_str() {
			"false" | "off" => return,
			"true" | "debug" | "on" => tracing::Level::DEBUG,
			"trace" => tracing::Level::TRACE,
			"info" => tracing::Level::INFO,
			"warn" => tracing::Level::WARN,
			"error" => tracing::Level::ERROR,
			_ => return,
		};

		let prefix_blacklist = &[
			"storage::db::mdbx",
			"alloy_transport_ipc",
			"trie",
			"engine",
			"reth::cli",
			"reth_tasks",
			"static_file",
			"txpool",
			"provider::static_file",
			"reth_node_builder::launch::common",
			"reth_db_common",
			"pruner",
			"reth_node_events",
		];

		tracing_subscriber::registry()
			.with(tracing_subscriber::fmt::layer())
			.with(filter_fn(move |metadata| {
				metadata.level() <= &level
					&& !prefix_blacklist
						.iter()
						.any(|prefix| metadata.target().starts_with(prefix))
			}))
			.init();
	}
}
