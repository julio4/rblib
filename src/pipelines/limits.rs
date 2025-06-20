use core::fmt::Debug;

/// This trait defines the limits that can be applied to the block building
/// process.
pub trait Limits: Debug + Send + Sync + 'static {}
