mod local;
mod signer;
mod step;
mod txs;
mod utils;

use alloy::primitives::Address;
pub use {
	local::LocalNode,
	signer::Signer,
	step::OneStep,
	txs::TransactionBuilder,
	utils::*,
};

/// Those accounts are defined in the gensis block of the test local node,
/// each prefunded with 100 ETH and nonces starting from 0.
pub struct FundedAccounts;

#[allow(clippy::cast_possible_truncation)]
impl FundedAccounts {
	const FUNDED_PRIVATE_KEYS: &[&str] = &[
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff81",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff82",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff83",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff84",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff85",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff86",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff87",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff88",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff89",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff90",
	];

	pub fn signer(key: u32) -> Signer {
		assert!(
			(key as usize) < Self::FUNDED_PRIVATE_KEYS.len(),
			"Key index out of bounds, must be less than {}",
			Self::FUNDED_PRIVATE_KEYS.len()
		);

		Signer::try_from_secret(
			Self::FUNDED_PRIVATE_KEYS[key as usize]
				.parse()
				.expect("invalid hardcoded builder private key"),
		)
		.expect("invalid hardcoded builder private key")
	}

	pub fn address(key: u32) -> Address {
		Self::signer(key).address
	}

	pub fn random() -> Signer {
		let key = rand::random::<u32>() % Self::FUNDED_PRIVATE_KEYS.len() as u32;
		Self::signer(key)
	}

	pub fn addresses() -> impl Iterator<Item = Address> {
		Self::FUNDED_PRIVATE_KEYS
			.iter()
			.enumerate()
			.map(|(i, _)| Self::address(i as u32))
	}
}

pub const ONE_ETH: u128 = 1_000_000_000_000_000_000;
pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 30_000_000;

#[macro_export]
macro_rules! make_step {
	($name:ident) => {
		#[derive(Debug, Clone)]
		pub struct $name;
		impl<P: Platform> Step<P> for $name {
			async fn step(
				self: std::sync::Arc<Self>,
				_: Checkpoint<P>,
				_: StepContext<P>,
			) -> ControlFlow<P> {
				todo!()
			}
		}
	};

	($name:ident, $state:ident) => {
		#[allow(dead_code)]
		#[derive(Debug, Clone)]
		pub struct $name($state);
		impl<P: Platform> Step<P> for $name {
			async fn step(
				self: std::sync::Arc<Self>,
				_: Checkpoint<P>,
				_: StepContext<P>,
			) -> ControlFlow<P> {
				todo!()
			}
		}
	};
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
