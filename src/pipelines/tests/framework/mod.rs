mod local;
mod signer;
mod txs;
mod utils;

pub use {local::LocalNode, signer::Signer, txs::TransactionBuilder, utils::*};

const BUILDER_PRIVATE_KEY: &str =
	"0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

const FUNDED_PRIVATE_KEYS: &[&str] =
	&["0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"];

pub const ONE_ETH: u128 = 1_000_000_000_000_000_000;
pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 30_000_000;

/// This gets invoked before any tests, when the cargo test framework loads the
/// test library.
#[ctor::ctor]
fn init_tests() {
	use tracing_subscriber::{filter::filter_fn, prelude::*};
	if let Ok(v) = std::env::var("TEST_TRACE") {
		let level = match v.as_str() {
			"false" | "off" => return,
			"true" | "debug" | "on" => tracing::Level::DEBUG,
			"trace" => tracing::Level::TRACE,
			"info" => tracing::Level::INFO,
			"warn" => tracing::Level::WARN,
			"error" => tracing::Level::ERROR,
			_ => return,
		};

		let prefix_blacklist = &["storage::db::mdbx", "alloy_transport_ipc"];

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
