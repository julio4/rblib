use crate::pipeline::{sealed, step::StepMode};

pub struct SimulatedPayload;
pub struct SimulatedContext;

pub struct Simulated;
impl StepMode for Simulated {
	type Context = SimulatedContext;
	type Payload = SimulatedPayload;
}
impl sealed::Sealed for Simulated {}
