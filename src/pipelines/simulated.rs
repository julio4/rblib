use super::{sealed, step::StepKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulatedPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulatedContext;

/// Simulated steps have access to the payload execution results and have the
/// state manipulation API attached to them.
#[derive(Debug)]
pub struct Simulated;

impl StepKind for Simulated {
	type Context = SimulatedContext;
	type Payload = SimulatedPayload;
}
impl sealed::Sealed for Simulated {}
