#![cfg_attr(not(test), no_std)]

mod payload;
// mod pipeline;

extern crate alloc;

pub use payload::*;
// pub use pipeline::*;

#[cfg(feature = "optimism")]
mod optimism {
	use {
		super::*,
		reth_optimism_node::{OpEngineTypes, OpEvmConfig},
	};

	pub struct Optimism;
	impl Platform for Optimism {
		type EngineTypes = OpEngineTypes;
		type EvmConfig = OpEvmConfig;
	}
}

#[cfg(feature = "optimism")]
pub use optimism::Optimism;

#[cfg(feature = "ethereum")]
mod ethereum {
	pub struct Ethereum;
}

#[cfg(feature = "ethereum")]
pub use ethereum::Ethereum;
