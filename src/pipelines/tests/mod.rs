//! Utilities used in unit tests

#[cfg(all(test, not(feature = "ethereum")))]
compile_error!("test builds require the `ethereum` feature to be enabled.");

mod framework;

#[cfg(test)]
mod revert;

#[cfg(test)]
mod smoke;

#[cfg(test)]
mod syntax;

pub use framework::*;
