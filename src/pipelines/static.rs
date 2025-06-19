use super::{sealed::Sealed, step::StepMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPayload;

pub struct StaticContext;

#[derive(Debug)]
pub struct Static;

impl StepMode for Static {
	type Context = StaticContext;
	type Payload = StaticPayload;
}

impl Sealed for Static {}
