use super::{sealed::Sealed, step::StepKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticContext;

#[derive(Debug)]
pub struct Static;

impl StepKind for Static {
	type Context = StaticContext;
	type Payload = StaticPayload;
}

impl Sealed for Static {}
