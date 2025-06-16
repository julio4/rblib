use crate::pipeline::{sealed::Sealed, step::StepMode};

pub struct StaticPayload;
pub struct StaticContext;

pub struct Static;

impl StepMode for Static {
	type Context = StaticContext;
	type Payload = StaticPayload;
}

impl Sealed for Static {}
