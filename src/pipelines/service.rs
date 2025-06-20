//! This module implements the machinery that takes a declarative pipeline
//! definition and turns it into a Reth compatible payload builder.

use {
	crate::*,
	alloc::boxed::Box,
	core::{
		marker::PhantomData,
		pin::Pin,
		task::{Context, Poll},
	},
	reth::{
		api::{FullNodeTypes, NodeTypes},
		builder::{components::PayloadServiceBuilder, BuilderContext},
		providers::CanonStateSubscriptions,
	},
	reth_payload_builder::{
		KeepPayloadJobAlive,
		PayloadBuilderError,
		PayloadBuilderHandle,
		PayloadBuilderService,
		PayloadJob as RethPayloadJobTrait,
		PayloadJobGenerator,
	},
	reth_transaction_pool::TransactionPool,
	tracing::{debug, info},
};

pub(crate) struct PipelineServiceBuilder<P: Platform>(
	pub Pipeline,
	pub PhantomData<P>,
);

impl<P, Node, Pool, EvmConfig> PayloadServiceBuilder<Node, Pool, EvmConfig>
	for PipelineServiceBuilder<P>
where
	P: Platform,
	Node: FullNodeTypes<Types = P::NodeTypes> + Send + Sync,
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

		let generator = JobGenerator::<P>(pipeline, PhantomData);

		let (service, builder) =
			PayloadBuilderService::<_, _, types::NodePayloadTypes<P>>::new(
				generator,
				ctx.provider().canonical_state_stream(),
			);

		ctx
			.task_executor()
			.spawn_critical("payload_builder_service", Box::pin(service));

		Ok(builder)
	}
}

/// This type is stored inside the [`PayloadBuilderService`] type in Reth.
/// There's one instance of this type per node and it is instantiated during the
/// node startup inside `spawn_payload_builder_service`.
///
/// The responsibility of this type is to respond to new payload requests when
/// FCU calls come from the CL Node. Each FCU call will generate a new PayloadID
/// on its side and will pass it to the `new_payload_job` method.
pub struct JobGenerator<P: Platform>(Pipeline, PhantomData<P>);

impl<P: Platform> PayloadJobGenerator for JobGenerator<P> {
	type Job = PayloadJob<P>;

	fn new_payload_job(
		&self,
		attr: <Self::Job as RethPayloadJobTrait>::PayloadAttributes,
	) -> Result<Self::Job, PayloadBuilderError> {
		info!("Creating new payload job with attributes: {attr:#?}");
		todo!()
	}
}

pub struct PayloadJob<P: Platform>(PhantomData<P>);

impl<P: Platform> RethPayloadJobTrait for PayloadJob<P> {
	type BuiltPayload = types::BuiltPayload<P>;
	type PayloadAttributes = types::PayloadBuilderAttributes<P>;
	type ResolvePayloadFuture = PayloadJobResolveFuture<P>;

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

impl<P: Platform> Future for PayloadJob<P> {
	type Output = Result<(), PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
		debug!("PayloadJob::poll");
		Poll::Pending
	}
}

pub enum PayloadJobResolveFuture<P: Platform> {
	Ready(Option<Result<types::BuiltPayload<P>, PayloadBuilderError>>),
}

impl<P: Platform> Future for PayloadJobResolveFuture<P> {
	type Output = Result<types::BuiltPayload<P>, PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
		info!("PayloadJobResolveFuture::poll");
		match self.get_mut() {
			PayloadJobResolveFuture::Ready(payload) => {
				if let Some(payload) = payload.take() {
					Poll::Ready(payload) // return the payload only once
				} else {
					Poll::Pending
				}
			}
		}
	}
}
