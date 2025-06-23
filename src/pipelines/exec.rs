//! Pipeline execution
//!
//! A pipeline is a declarative description of the steps that need to be
//! executed to arrive at a desired payload or signal the inability to do so.
//!
//! The pipeline definition itself does not have any logic for executing
//! steps, enforcing limits or nested pipelines. This is done by the execution
//! engine.
//!
//! Before running the pipeline, the payload will have the platform-specific
//! [`BlockBuilder::apply_pre_execution_changes`] applied to its state.

use {
	crate::{
		pipelines::service::ServiceContext,
		traits,
		types,
		BlockContext,
		Pipeline,
		Platform,
	},
	core::{
		pin::Pin,
		task::{Context, Poll},
	},
	reth_payload_builder::PayloadBuilderError,
	std::sync::Arc,
	tracing::info,
};

pub(super) struct PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	pipeline: Arc<Pipeline>,
	block: Arc<BlockContext<P>>,
	service: Arc<ServiceContext<P, Provider, Pool>>,
}

impl<
		P: Platform,
		Provider: traits::ProviderBounds<P>,
		Pool: traits::PoolBounds<P>,
	> PipelineExecutor<P, Provider, Pool>
{
	pub fn run(
		pipeline: Arc<Pipeline>,
		block: Arc<BlockContext<P>>,
		service: Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self {
		info!("pipeline execution started");

		Self {
			pipeline,
			block,
			service,
		}
	}

	pub fn pipeline(&self) -> &Pipeline {
		&self.pipeline
	}

	pub fn block(&self) -> &BlockContext<P> {
		&self.block
	}

	pub fn service(&self) -> &ServiceContext<P, Provider, Pool> {
		&self.service
	}

	pub fn execute(
		&mut self,
	) -> Result<types::BuiltPayload<P>, PayloadBuilderError> {
		// Here we would implement the logic to execute the pipeline steps
		// and handle nested pipelines, limits, etc.
		// This is a placeholder for the actual execution logic.
		info!("Pipeline execution resolved");
		build_empty_payload(self)
	}
}

impl<P, Provider, Pool> Future for PipelineExecutor<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	type Output = Result<types::BuiltPayload<P>, ClonablePayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
		let executor = self.get_mut();
		Poll::Ready(executor.execute().map_err(ClonablePayloadBuilderError))
	}
}

fn build_empty_payload<P, Provider, Pool>(
	executor: &mut PipelineExecutor<P, Provider, Pool>,
) -> Result<types::BuiltPayload<P>, PayloadBuilderError>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	// This function should build an empty payload based on the pipeline and
	// attributes. For now, we return a placeholder.
	info!(
		"Building empty payload for pipeline: {} and attributes: {:?}",
		executor.pipeline(),
		executor.block().attributes()
	);

	// start a new payload, don't add any transactions and build it immediately.
	let checkpoint = executor.block().start();
	P::into_built_payload(
		checkpoint,
		executor.service().pool(),
		executor.service().provider(),
	)
}

/// This is a hack to allow cloning of the `PayloadBuilderError`.
/// Because the same result of this future is cached and reuesed
/// in the `PayloadJob`, we need to be able to clone the error so
/// we can use it with `futures::Shared`
pub(super) struct ClonablePayloadBuilderError(PayloadBuilderError);

impl Clone for ClonablePayloadBuilderError {
	fn clone(&self) -> Self {
		match &self.0 {
			PayloadBuilderError::MissingParentHeader(fixed_bytes) => {
				Self(PayloadBuilderError::MissingParentHeader(*fixed_bytes))
			}
			PayloadBuilderError::MissingParentBlock(fixed_bytes) => {
				Self(PayloadBuilderError::MissingParentBlock(*fixed_bytes))
			}
			PayloadBuilderError::ChannelClosed => {
				Self(PayloadBuilderError::ChannelClosed)
			}
			PayloadBuilderError::MissingPayload => {
				Self(PayloadBuilderError::MissingPayload)
			}
			PayloadBuilderError::Internal(reth_error) => {
				Self(PayloadBuilderError::Internal(todo!()))
			}
			PayloadBuilderError::EvmExecutionError(error) => {
				todo!()
			}
			PayloadBuilderError::Other(error) => todo!(),
		}
	}
}

impl From<ClonablePayloadBuilderError> for PayloadBuilderError {
	fn from(error: ClonablePayloadBuilderError) -> Self {
		error.0
	}
}
