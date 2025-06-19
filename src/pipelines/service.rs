//! This module implements the machinery that takes a declarative pipeline
//! definition and turns it into a Reth compatible payload builder.

use {
	crate::Pipeline,
	core::{
		pin::Pin,
		task::{Context, Poll},
	},
	reth::{
		api::{FullNodeTypes, NodeTypes},
		builder::{components::PayloadServiceBuilder, BuilderContext},
	},
	reth_payload_builder::{
		KeepPayloadJobAlive,
		PayloadBuilderError,
		PayloadBuilderHandle,
		PayloadJob as RethPayloadJobTrait,
		PayloadJobGenerator,
	},
	reth_transaction_pool::TransactionPool,
	tracing::debug,
};

pub(crate) struct PipelineServiceBuilder(pub Pipeline);

impl<Node, Pool, EvmConfig> PayloadServiceBuilder<Node, Pool, EvmConfig>
	for PipelineServiceBuilder
where
	Node: FullNodeTypes + Send + Sync,
	Pool: TransactionPool + Send + Sync,
	EvmConfig: Send + Sync,
{
	async fn spawn_payload_builder_service(
		self,
		ctx: &BuilderContext<Node>,
		pool: Pool,
		evm_config: EvmConfig,
	) -> eyre::Result<PayloadBuilderHandle<<Node::Types as NodeTypes>::Payload>>
	{
		let pipeline = self.0;
		debug!("Spawning payload builder: {pipeline:#?}");

		let generator = JobGenerator(pipeline);
		todo!()
	}
}

/// This type is stored inside the [`PayloadBuilderService`] type in Reth.
/// There's one instance of this type per node and it is instantiated during the
/// node startup inside `spawn_payload_builder_service`.
///
/// The responsibility of this type is to respond to new payload requests when
/// FCU calls come from the CL Node. Each FCU call will generate a new PayloadID
/// on its side and will pass it to the `new_payload_job` method.
pub struct JobGenerator(Pipeline);

impl PayloadJobGenerator for JobGenerator {
	type Job = PayloadJob;

	fn new_payload_job(
		&self,
		attr: <Self::Job as RethPayloadJobTrait>::PayloadAttributes,
	) -> Result<Self::Job, PayloadBuilderError> {
		todo!()
	}
}

pub struct PayloadJob;

impl RethPayloadJobTrait for PayloadJob {
	type BuiltPayload;
	type PayloadAttributes;
	type ResolvePayloadFuture;

	fn best_payload(
		&self,
	) -> Result<Self::BuiltPayload, reth_payload_builder::PayloadBuilderError> {
		todo!()
	}

	fn payload_attributes(
		&self,
	) -> Result<Self::PayloadAttributes, reth_payload_builder::PayloadBuilderError>
	{
		todo!()
	}

	fn resolve_kind(
		&mut self,
		kind: reth_payload_builder::PayloadKind,
	) -> (Self::ResolvePayloadFuture, KeepPayloadJobAlive) {
		todo!()
	}
}

impl Future for PayloadJob {
	type Output = Result<(), PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
		debug!("PayloadJob::poll");
		Poll::Pending
	}
}
