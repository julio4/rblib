use {
	super::traits,
	crate::{
		pipelines::{
			exec::{ClonablePayloadBuilderError, PipelineExecutor},
			service::ServiceContext,
		},
		prelude::*,
		reth::{
			api::PayloadBuilderAttributes,
			payload::builder::{PayloadJob as RethPayloadJobTrait, *},
		},
	},
	core::{
		pin::Pin,
		task::{Context, Poll},
	},
	futures::{FutureExt, future::Shared},
	std::sync::Arc,
	tracing::{debug, info},
};

/// This type is responsible for handling one payload generation request.
///
/// A payload generation request begins when the CL node sends us FCU request
/// with payload attributes. That will trigger
/// [`PayloadJobGenerator::new_payload_job`] which in turn will create a new
/// instance of this type.
///
/// This is a long-running job that will be polled by the CL node until it is
/// resolved. The job future must resolve within 1 second from the moment
/// [`PayloadJob::resolve_kind`] is called with [`PaylodKind::Earliest`].
pub struct PayloadJob<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	block: BlockContext<P>,
	fut: ExecutorFuture<P, Provider, Pool>,
}

impl<P, Provider, Pool> PayloadJob<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	pub fn new(
		pipeline: &Arc<Pipeline<P>>,
		block: BlockContext<P>,
		service: &Arc<ServiceContext<P, Provider, Pool>>,
	) -> Self {
		let fut = ExecutorFuture::new(PipelineExecutor::run(
			Arc::clone(pipeline),
			block.clone(),
			Arc::clone(service),
		));

		debug!("Creating new PayloadJob with block context: {block:#?}");

		Self { block, fut }
	}
}

impl<P, Provider, Pool> RethPayloadJobTrait for PayloadJob<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	type BuiltPayload = types::BuiltPayload<P>;
	type PayloadAttributes = types::PayloadBuilderAttributes<P>;
	type ResolvePayloadFuture = ExecutorFuture<P, Provider, Pool>;

	fn best_payload(&self) -> Result<Self::BuiltPayload, PayloadBuilderError> {
		unimplemented!("PayloadJob::best_payload is not implemented");
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
			// This is called when the CL node sends us a GetPayload EngineAPI
			// request. The time between the call to `resolve_kind` and the
			// creation of the payload job is the block time.
			// The future that is returned here must resolve within 1 second per eth
			// protocol.
			PayloadKind::Earliest => {
				info!(
					"Resolving earliest payload for job {}",
					self.block.attributes().payload_id()
				);
				(self.fut.clone(), KeepPayloadJobAlive::No)
			}
			PayloadKind::WaitForPending => {
				unimplemented!("PayloadKind::WaitForPending mode not supported");
			}
		}
	}
}

/// This future is polled for the first time by the Reth runtime when the
/// `PayloadJob` is created. Here we want to immediately start executing
/// the pipeline instead of waiting for the `resolve_kind` to be called.
impl<P, Provider, Pool> Future for PayloadJob<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	type Output = Result<(), PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		// When the payload job future is polled, we begin executing the pipeline
		// production future immediatelly as well, so that the time between the
		// creation of the job and the call to `resolve_kind` is utilized for the
		// pipeline execution.
		//
		// PayloadJob is going to be dropped anyway when the `ResolvePayloadFuture`
		// is resolved.
		//
		// At this point we don't return the result of the pipeline execution, but
		// we want to ensure that any errors that occur during the pipeline
		// execution are propagated and stop they payload job future immediately.
		if let Poll::Ready(Err(e)) = self.get_mut().fut.poll_unpin(cx) {
			return Poll::Ready(Err(e));
		}

		// On happy paths or in-progress pipielines, keep the future alive. Reth
		// will drop it when a payload is resolved.
		Poll::Pending
	}
}

/// This future wraps the `PipelineExecutor` and is used to poll the
/// internal executor of the pipeline. Once this future is resolved, it
/// can be polled again and will return copie of the resolved payload.
pub struct ExecutorFuture<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	state: ExecutorFutureState<P, Provider, Pool>,
}

/// This enum allows us to wrap the `PipelineExecutor` future
/// and cache the result of the execution. Also it makes the executor future
/// clonable, so that many copies of the future could be returned from
/// `resolve_kind`.
///
/// Whenever any of the copies of the future is polled, it will poll the
/// executor, if any copy resolved, all copies will also resolve with the same
/// result.
enum ExecutorFutureState<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	Ready(Result<types::BuiltPayload<P>, ClonablePayloadBuilderError>),
	Future(Shared<PipelineExecutor<P, Provider, Pool>>),
}

impl<P, Provider, Pool> ExecutorFuture<P, Provider, Pool>
where
	P: Platform,
	Pool: traits::PoolBounds<P>,
	Provider: traits::ProviderBounds<P>,
{
	pub fn new(executor: PipelineExecutor<P, Provider, Pool>) -> Self {
		Self {
			state: ExecutorFutureState::Future(executor.shared()),
		}
	}
}

impl<P, Provider, Pool> Future for ExecutorFuture<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	type Output = Result<types::BuiltPayload<P>, PayloadBuilderError>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = self.get_mut();
		match this.state {
			// we already have a result from previous polls, return a clone of it
			ExecutorFutureState::Ready(ref result) => Poll::Ready(result.clone()),

			// we are still in progress. keep polling the inner executor future.
			ExecutorFutureState::Future(ref mut executor) => {
				match executor.poll_unpin(cx) {
					Poll::Ready(result) => {
						// got a result. All future polls will return the result directly
						// without polling the executor again.
						this.state = ExecutorFutureState::Ready(result.clone());
						Poll::Ready(result)
					}
					Poll::Pending => Poll::Pending,
				}
			}
		}
		.map_err(Into::into)
	}
}

/// We want this to be clonable because the `resolve_kind` method could
/// potentially return multiple copies of the future, and we want all of them to
/// resolve with the same result at the same time.
impl<P, Provider, Pool> Clone for ExecutorFuture<P, Provider, Pool>
where
	P: Platform,
	Provider: traits::ProviderBounds<P>,
	Pool: traits::PoolBounds<P>,
{
	fn clone(&self) -> Self {
		Self {
			state: match &self.state {
				ExecutorFutureState::Ready(result) => {
					ExecutorFutureState::Ready(result.clone())
				}
				ExecutorFutureState::Future(future) => {
					ExecutorFutureState::Future(future.clone())
				}
			},
		}
	}
}
