use {
	super::{sealed, step::StepKind},
	crate::{Checkpoint, Platform},
};

/// Simulated steps have access to the payload execution results and have the
/// state manipulation API attached to them.
#[derive(Debug)]
pub struct Simulated;
impl sealed::Sealed for Simulated {}
impl StepKind for Simulated {
	type Payload<P: Platform> = SimulatedPayload<P>;
}

#[derive(Debug)]
pub struct SimulatedPayload<P: Platform> {
	checkpoint: Checkpoint<P>,
}

impl<P: Platform> SimulatedPayload<P> {
	/// Creates a new simulated payload with the given checkpoint.
	pub fn new(checkpoint: Checkpoint<P>) -> Self {
		Self { checkpoint }
	}
}
