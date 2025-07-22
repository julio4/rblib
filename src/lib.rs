mod payload;
mod pipelines;
mod platform;

// Public API exports
pub use {payload::*, pipelines::*, platform::*};

// Exported test utilities for use in tests in this and other crates.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// Reexport reth version that is used by rblib as a convenience for downstream
// users. Those exports should be enough to get started with a simple node.
pub mod reth {
	pub use reth_origin::*;

	pub mod cli {
		pub use {reth_cli::*, reth_origin::cli::*};
	}

	pub mod evm {
		pub use reth_evm::*;
	}

	pub mod errors {
		pub use reth_errors::*;
	}

	pub mod payload {
		pub use ::reth_origin::payload::*;
		pub mod builder {
			pub use {
				reth_basic_payload_builder::*,
				reth_ethereum_payload_builder::*,
				reth_payload_builder::*,
			};
		}
	}

	pub mod node {
		pub mod builder {
			pub use reth_node_builder::*;
		}

		pub mod transaction_pool {
			pub use reth_transaction_pool::*;
		}
	}

	pub mod ethereum {
		pub use reth_ethereum::*;
	}

	#[cfg(feature = "optimism")]
	pub mod optimism {
		pub mod chainspec {
			pub use reth_optimism_chainspec::*;
		}
		pub mod node {
			pub use reth_optimism_node::*;
		}
		pub mod forks {
			pub use reth_optimism_forks::*;
		}
		pub mod cli {
			pub use reth_optimism_cli::*;
		}
		pub mod primitives {
			pub use reth_optimism_primitives::*;
		}
	}
}

pub mod alloy {
	pub use alloy_origin::*;

	pub mod evm {
		pub use alloy_evm::*;
	}

	#[cfg(any(test, feature = "test-utils"))]
	pub mod genesis {
		pub use alloy_genesis::*;
	}

	#[cfg(feature = "optimism")]
	pub mod optimism {
		pub use op_alloy::*;
	}
}

pub use uuid;
