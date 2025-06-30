use {
	super::{sealed::Sealed, step::StepKind},
	crate::{types, Platform},
};

/// This type is used to represent a static step in the pipeline.
///
/// Static steps do not execute their transactions and do not have access to
/// previous execution results. They only receive and produce static lists of
/// transactions.
#[derive(Debug, Clone, Copy)]
pub struct Static;
impl Sealed for Static {}
impl StepKind for Static {
	type Payload<P: Platform> = StaticPayload<P>;
}

/// A static payload is just a vector of transactions, no extra data about the
/// result of the execution are available.
pub type StaticPayload<P> = Vec<types::Transaction<P>>;
