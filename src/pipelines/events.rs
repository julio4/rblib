use {super::*, core::marker::PhantomData};

#[derive(Clone, Default, Debug)]
pub struct EventsBus<P: Platform>(Arc<EventsBusInner<P>>);

impl<P: Platform> EventsBus<P> {
	pub fn publish<E>(&self, event: E)
	where
		E: Clone + Any + Send + Sync + 'static,
	{
		self.0.publish(event);
	}
}

#[derive(Default, Debug)]
struct EventsBusInner<P: Platform> {
	_phantom: PhantomData<P>,
}

impl<P: Platform> EventsBusInner<P> {
	pub fn publish<E>(&self, _event: E)
	where
		E: Clone + Any + Send + Sync + 'static,
	{
		// todo
	}
}
