//! Payload building API
//!
//! This API is used to construct individual payloads.

mod block;
mod checkpoint;
mod ext;
mod span;

pub use {
	block::{BlockContext, Error as BlockError},
	checkpoint::{Checkpoint, Error as CheckpointError},
	ext::{CheckpointExt, SpanExt},
	span::{Error as SpanError, Span},
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum StateError {
	#[error("Provider error: {0}")]
	Provider(#[from] crate::reth::errors::ProviderError),
}

impl crate::reth::revm::db::DBErrorMarker for StateError {}

#[cfg(test)]
mod tests;
