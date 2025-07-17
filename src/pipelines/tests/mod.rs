//! Utilities used in unit tests

#[cfg(all(test, not(feature = "optimism")))]
compile_error!("test builds require the `optimism` feature to be enabled.");

mod framework;

#[cfg(test)]
mod revert;

#[cfg(test)]
mod smoke;

#[cfg(test)]
mod syntax;

pub use framework::*;
