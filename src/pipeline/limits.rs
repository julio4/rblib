use core::fmt::Debug;

pub trait Limits: Debug + Send + Sync + 'static {}
