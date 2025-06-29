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

/// A simulated payload is a state checkpoint that contains a history of all
/// state mutations that were applied to it so far. It can be used to create new
/// versions of the payload by applying new transactions to it or going back to
/// previous checkpoints.
pub type SimulatedPayload<P> = Checkpoint<P>;
