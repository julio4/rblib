use {
	crate::Platform,
	alloy::{
		consensus::{SignableTransaction, Signed},
		signers::Signature,
	},
	reth_ethereum::primitives::SignedTransaction,
	reth_payload_builder::PayloadId,
};
// public test utils exports
pub use {accounts::FundedAccounts, node::LocalNode, step::OneStep, utils::*};

mod accounts;
mod node;
mod step;
mod utils;

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

/// Types implementing this trait emulate a consensus node and are responsible
/// for generating `ForkChoiceUpdate`, `GetPayload`, `SetPayload` and other
/// Engine API calls to trigger payload building and canonical chain updates.
pub trait ConsensusDriver<P: Platform + NetworkSelector>:
	Sized + Unpin + Send + Sync + 'static
{
	type Params: Default;

	/// Starts the process of building a new block on the node.
	///
	/// This should run the engine api CL <-> EL protocol up to the point of
	/// scheduling the payload build process and receiving a payload ID.
	fn start_building(
		&self,
		node: &LocalNode<P, Self>,
		target_timestamp: u64,
		params: &Self::Params,
	) -> impl Future<Output = eyre::Result<PayloadId>>;

	/// This is always called after a payload building process has successfully
	/// started and a payload ID has been returned.
	///
	/// This should run the engine api CL <-> EL protocol to retreive the newly
	/// built payload then set it as the canonical payload on the node.
	fn finish_building(
		&self,
		node: &LocalNode<P, Self>,
		payload_id: PayloadId,
		params: &Self::Params,
	) -> impl Future<Output = eyre::Result<select::BlockResponse<P>>>;
}

/// This trait is used to automatically select the correct local test node type
/// based on the platform that is being tested.
pub trait TestNodeFactory<P: crate::Platform + NetworkSelector> {
	type ConsensusDriver: ConsensusDriver<P>;

	fn create_test_node(
		pipeline: crate::Pipeline<P>,
	) -> impl Future<Output = eyre::Result<node::LocalNode<P, Self::ConsensusDriver>>>;
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
