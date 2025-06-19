//! Payload building API
//!
//! This API is used to construct individual payloads.

mod block;
mod checkpoint;
mod platform;
mod span;

pub use {
	block::BlockContext,
	checkpoint::Checkpoint,
	platform::Platform,
	span::Span,
};

#[cfg(feature = "optimism")]
mod optimism;

#[cfg(feature = "optimism")]
pub use optimism::Optimism;

#[cfg(feature = "ethereum")]
mod ethereum;

#[cfg(feature = "ethereum")]
pub use ethereum::Ethereum;

#[cfg(test)]
mod tests;
