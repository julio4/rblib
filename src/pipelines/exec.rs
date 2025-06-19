//! Pipeline execution
//!
//! A pipeline is a declarative description of the steps that need to be
//! executed to arrive at a desired payload or signal the inability to do so.
//!
//! The pipeline definition itself does not have any logic for executing
//! steps, enforcing limits or nested pipelines. This is done by the execution
//! engine.
//!
//! The execution engine starts with an empty payload with no transactions in
//! it. The empty payload needs to be populated with block metadata first,
//! namely:
//!  - The parent block header
//!  - The PayloadAttributes supplied by the CL client
//!  - ChainSpec
//!  - Current blockchain state rooted at the parent block.

use crate::*;

struct ExecContext;

pub async fn run(pipeline: Pipeline) {
	let mut _pipeline = pipeline;
}
