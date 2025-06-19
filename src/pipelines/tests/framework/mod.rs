mod local;

pub use local::LocalNode;

/// This gets invoked before any tests, when the cargo test framework loads the
/// test library. It injects itself into
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

		// let prefix_blacklist = &["alloy_transport_ipc", "storage::db::mdbx"];
		let prefix_blacklist = &["storage::db::mdbx"];

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
