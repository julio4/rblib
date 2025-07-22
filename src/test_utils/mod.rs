//! rblib testing infrastructure
//!
//! Those are utilities that provide ways to test steps, pipelines, platforms
//! and other primitives defined by rblib and downstream crates extending and
//! building on top of rblib.
//!
//! By default, two test platforms are defined for the two platforms that ship
//! with rblib out of the box:
//! - [`Ethereum`]
//! - [`Optimism`]
//!
//! If you want to make your own [`crate::Platform`] type implementation
//! available for testing with those utils you need to implement the
//! [`TestablePlatform`] trait for it.

mod accounts;
mod ethereum;
mod exts;
mod node;
mod platform;
mod step;

#[cfg(feature = "optimism")]
mod optimism;

pub use {
	accounts::{FundedAccounts, WithFundedAccounts},
	exts::*,
	node::{ConsensusDriver, LocalNode},
	platform::{NetworkSelector, TestNodeFactory, TestablePlatform, select},
	rblib_tests_macros::{if_platform, rblib_test},
	step::OneStep,
};

pub const ONE_ETH: u128 = 1_000_000_000_000_000_000;
pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 30_000_000;

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
