use {
	super::traits,
	crate::{pipelines::service::ServiceContext, *},
	alloc::sync::Arc,
	core::{
		pin::Pin,
		task::{Context, Poll},
	},
	reth::api::PayloadBuilderAttributes,
	reth_payload_builder::{PayloadJob as RethPayloadJobTrait, *},
	tracing::info,
};

/// This type is stored inside the [`PayloadBuilderService`] type in Reth.
/// There's one instance of this type per node and it is instantiated during the
/// node startup inside `spawn_payload_builder_service`.
///
/// The responsibility of this type is to respond to new payload requests when
/// FCU calls come from the CL Node. Each FCU call will generate a new PayloadID
/// on its side and will pass it to the `new_payload_job` method.
pub struct JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pipeline: Arc<Pipeline>,
	service: Arc<ServiceContext<Plat, Provider, Pool>>,
}

impl<Plat, Provider, Pool> JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	pub fn new(
		pipeline: Arc<Pipeline>,
		service: Arc<ServiceContext<Plat, Provider, Pool>>,
	) -> Self {
		info!("Creating new JobGenerator with pipeline: {pipeline:#?}");
		Self { pipeline, service }
	}
}

impl<Plat, Provider, Pool> PayloadJobGenerator
	for JobGenerator<Plat, Provider, Pool>
where
	Plat: Platform,
	Provider: traits::ProviderBounds<Plat>,
	Pool: traits::PoolBounds<Plat>,
{
	type Job = PayloadJob<Plat>;

	fn new_payload_job(
		&self,
		attribs: <Self::Job as RethPayloadJobTrait>::PayloadAttributes,
	) -> Result<Self::Job, PayloadBuilderError> {
		info!("PayloadJobGenerator::new_payload_job {attribs:#?}");
		let header = if attribs.parent().is_zero() {
			self.service.provider().latest_header()?.ok_or_else(|| {
				PayloadBuilderError::MissingParentHeader(attribs.parent())
			})
		} else {
			self
				.service
				.provider()
				.sealed_header_by_hash(attribs.parent())?
				.ok_or_else(|| {
					PayloadBuilderError::MissingParentHeader(attribs.parent())
				})
		}?;

		let base_state =
			self.service.provider().state_by_block_hash(header.hash())?;

		let block_ctx = BlockContext::new(
			header,
			attribs,
			base_state,
			self.service.evm_config().clone(),
			self.service.chain_spec().as_ref(),
		)
		.map_err(PayloadBuilderError::other)?;

		Ok(PayloadJob::new(Arc::clone(&self.pipeline), block_ctx))
	}
}

pub struct PayloadJob<P: Platform> {
	pipeline: Arc<Pipeline>,
	block: BlockContext<P>,
}

impl<P: Platform> PayloadJob<P> {
	pub fn new(pipeline: Arc<Pipeline>, block: BlockContext<P>) -> Self {
		info!("Creating new PayloadJob with block context: {block:#?}");
		Self { pipeline, block }
	}
}

impl<P: Platform> RethPayloadJobTrait for PayloadJob<P> {
	type BuiltPayload = types::BuiltPayload<P>;
	type PayloadAttributes = types::PayloadBuilderAttributes<P>;
	type ResolvePayloadFuture = PayloadJobResolveFuture<P>;

	fn best_payload(&self) -> Result<Self::BuiltPayload, PayloadBuilderError> {
		todo!("PayloadJob::best_payload")
	}

	fn payload_attributes(
		&self,
	) -> Result<Self::PayloadAttributes, PayloadBuilderError> {
		Ok(self.block.attributes().clone())
	}

	fn resolve_kind(
		&mut self,
		kind: PayloadKind,
	) -> (Self::ResolvePayloadFuture, KeepPayloadJobAlive) {
		match kind {
			PayloadKind::Earliest => {
				info!(
					"Resolving earliest payload for job {}",
					self.block.attributes().payload_id()
				);
				(
					PayloadJobResolveFuture::Ready(Some(Ok(build_empty_payload(
						&self.pipeline,
						&self.block,
					)))),
					KeepPayloadJobAlive::No,
				)
			}
			PayloadKind::WaitForPending => {
				info!(
					"Waiting for pending payload for job {}",
					self.block.attributes().payload_id()
				);
				(
					PayloadJobResolveFuture::Ready(None),
					KeepPayloadJobAlive::Yes,
				)
			}
		}
	}
}

impl<P: Platform> Future for PayloadJob<P> {
	type Output = Result<(), PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
		info!("PayloadJob::poll");
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

fn build_empty_payload<P: Platform>(
	pipeline: &Pipeline,
	block_ctx: &BlockContext<P>,
) -> types::BuiltPayload<P> {
	// This function should build an empty payload based on the pipeline and
	// attributes. For now, we return a placeholder.
	info!("Building empty payload for pipeline: {:#?}", pipeline);

	// start a new payload, don't add any transactions and build it immediately.
	block_ctx.start().build().unwrap()
}
