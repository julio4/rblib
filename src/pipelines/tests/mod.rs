//! Utilities used in unit tests

#[cfg(all(test, feature = "pipelines", not(feature = "ethereum")))]
compile_error!(
	"Pipelines test builds require the `ethereum` feature to be enabled."
);

mod framework;

#[cfg(test)]
mod revert;

#[cfg(test)]
mod smoke;

#[cfg(test)]
mod syntax;

pub use framework::*;
