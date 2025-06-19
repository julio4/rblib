use super::{sealed, step::StepMode};

pub struct SimulatedPayload;
pub struct SimulatedContext;

/// Simulated steps have access to the payload execution results and have the
/// state manipulation API attached to them.
#[derive(Debug)]
pub struct Simulated;

impl StepMode for Simulated {
	type Context = SimulatedContext;
	type Payload = SimulatedPayload;
}
impl sealed::Sealed for Simulated {}
