use {
	super::*,
	core::{any::TypeId, marker::PhantomData},
	dashmap::{DashMap, mapref::one::Ref},
	tokio::sync::broadcast::Sender,
	tokio_stream::{StreamExt, wrappers::BroadcastStream},
};

/// This type is responsible for propagating events emitted by steps in the
/// pipeline.
///
/// Notes:
///  - In a pipeline with nested pipelines, the top-level pipline's event bus is
///    responsible for handling all events of all contained pipelines and steps.
#[derive(Default, Debug)]
pub struct EventsBus<P: Platform> {
	publishers: DashMap<TypeId, Sender<Arc<dyn Any + Send + Sync>>>,
	phantom: PhantomData<P>,
}

impl<P: Platform> EventsBus<P> {
	/// Publishes an event of type `E` to all current subscribers.
	/// Each subscriber will receive a clone of the event.
	pub fn publish<E>(&self, event: E)
	where
		E: Clone + Any + Send + Sync + 'static,
	{
		let _ = self.sender::<E>().send(Arc::new(event));
	}

	/// Returns a stream that yields events of type `E`.
	pub fn subscribe<E>(&self) -> impl Stream<Item = E> + Send + Sync + 'static
	where
		E: Clone + Any + Send + Sync + 'static,
	{
		BroadcastStream::new(self.sender::<E>().subscribe()).filter_map(|event| {
			let event = event.ok()?;
			let casted = event.downcast::<E>().ok()?;
			Some((*casted).clone())
		})
	}

	fn sender<E>(&self) -> Ref<'_, TypeId, Sender<Arc<dyn Any + Send + Sync>>>
	where
		E: Clone + Any + Send + Sync + 'static,
	{
		self
			.publishers
			.entry(TypeId::of::<E>())
			.or_insert_with(|| Sender::new(100))
			.downgrade()
	}
}

#[cfg(test)]
mod tests {
	use {super::*, crate::test_utils::*, futures::StreamExt};

	#[derive(Clone, Debug, PartialEq, Eq)]
	struct StringEvent(String);

	#[derive(Clone, Debug, PartialEq, Eq)]
	struct UInt32Event(u32);

	struct StepEmittingOneTypeOfEvent;
	impl<P: Platform> Step<P> for StepEmittingOneTypeOfEvent {
		async fn before_job(
			self: Arc<Self>,
			ctx: StepContext<P>,
		) -> Result<(), PayloadBuilderError> {
			ctx.emit(StringEvent("before job".to_string()));
			Ok(())
		}

		async fn step(
			self: Arc<Self>,
			payload: Checkpoint<P>,
			ctx: StepContext<P>,
		) -> ControlFlow<P> {
			ctx.emit(StringEvent("inside step".to_string()));
			ControlFlow::Ok(payload)
		}

		async fn after_job(
			self: Arc<Self>,
			ctx: StepContext<P>,
			_: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
		) -> Result<(), PayloadBuilderError> {
			ctx.emit(StringEvent("after job".to_string()));
			Ok(())
		}
	}

	struct StepEmittingTwoTypesOfEvents;
	impl<P: Platform> Step<P> for StepEmittingTwoTypesOfEvents {
		async fn before_job(
			self: Arc<Self>,
			ctx: StepContext<P>,
		) -> Result<(), PayloadBuilderError> {
			ctx.emit(StringEvent("before job".to_string()));
			ctx.emit(UInt32Event(50));
			Ok(())
		}

		async fn step(
			self: Arc<Self>,
			payload: Checkpoint<P>,
			ctx: StepContext<P>,
		) -> ControlFlow<P> {
			ctx.emit(StringEvent("inside step".to_string()));
			ctx.emit(UInt32Event(100));
			ctx.emit(UInt32Event(101));
			ControlFlow::Ok(payload)
		}

		async fn after_job(
			self: Arc<Self>,
			ctx: StepContext<P>,
			_: Arc<Result<types::BuiltPayload<P>, PayloadBuilderError>>,
		) -> Result<(), PayloadBuilderError> {
			ctx.emit(StringEvent("after job".to_string()));
			ctx.emit(UInt32Event(200));
			Ok(())
		}
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn one_event_type_is_propagated<P: TestablePlatform>() {
		let step = OneStep::<P>::new(StepEmittingOneTypeOfEvent);

		let mut string_subs = step.subscribe::<StringEvent>();

		let output = step.run().await;
		assert!(output.is_ok());

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("before job".to_string()))
		);

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("inside step".to_string()))
		);

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("after job".to_string()))
		);
	}

	#[rblib_test(Ethereum, Optimism)]
	async fn two_event_types_are_propagated<P: TestablePlatform>() {
		let step = OneStep::<P>::new(StepEmittingTwoTypesOfEvents);

		let mut string_subs = step.subscribe::<StringEvent>();
		let mut uint32_subs = step.subscribe::<UInt32Event>();

		let output = step.run().await;
		assert!(output.is_ok());

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("before job".to_string()))
		);

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("inside step".to_string()))
		);

		assert_eq!(
			string_subs.next().await,
			Some(StringEvent("after job".to_string()))
		);
		assert!(string_subs.next().await.is_none());

		assert_eq!(uint32_subs.next().await, Some(UInt32Event(50)));
		assert_eq!(uint32_subs.next().await, Some(UInt32Event(100)));
		assert_eq!(uint32_subs.next().await, Some(UInt32Event(101)));
		assert_eq!(uint32_subs.next().await, Some(UInt32Event(200)));
		assert!(uint32_subs.next().await.is_none());
	}
}
