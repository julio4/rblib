use {
	super::{types, Checkpoint, Platform},
	alloc::sync::Arc,
	reth::{
		api::PayloadBuilderAttributes,
		payload::PayloadId,
		primitives::SealedHeader,
		providers::StateProviderBox,
	},
};

/// This is a factory type that can create different parallel streams of
/// incrementally built payloads, all rooted at the same parent block.
///
/// Usually instances of this type are created as a response to CL client
/// ForkchoiceUpdated requests with PayloadAttributes that signals the need
/// to start constructing a new payload on top of a given block.
///
/// This type is cheap to clone.
pub struct BlockContext<P: Platform> {
	inner: Arc<BlockContextInner<P>>,
}

impl<P: Platform> Clone for BlockContext<P> {
	fn clone(&self) -> Self {
		Self {
			inner: Arc::clone(&self.inner),
		}
	}
}

/// Constructors
impl<P: Platform> BlockContext<P> {
	pub fn new(
		parent: SealedHeader<types::Header<P>>,
		attribs: types::PayloadBuilderAttributes<P>,
		base_state: StateProviderBox,
	) -> Self {
		Self {
			inner: Arc::new(BlockContextInner {
				parent,
				attribs,
				base_state,
			}),
		}
	}
}

/// Public Read API
impl<P: Platform> BlockContext<P> {
	/// Returns the parent block header of the block for which the payload is
	/// being built.
	pub fn parent(&self) -> &SealedHeader<types::Header<P>> {
		&self.inner.parent
	}

	/// Returns the payload attributes that were supplied by the CL client
	/// during the ForkchoiceUpdated request.
	pub fn attributes(&self) -> &types::PayloadBuilderAttributes<P> {
		&self.inner.attribs
	}

	/// Returns the payload ID that was supplied by the CL client
	/// during the ForkchoiceUpdated request inside the payload attributes.
	pub fn payload_id(&self) -> PayloadId {
		self.attributes().payload_id()
	}

	/// Returns the state provider that provides access to the state of the
	/// environment rooted at the end of the parent block for which the payload
	/// is being built.
	pub fn base_state(&self) -> &StateProviderBox {
		&self.inner.base_state
	}
}

/// Public building API
impl<P: Platform> BlockContext<P> {
	/// Creates a new empty payload checkpoint rooted at the state of the parent
	/// block. Instances of the `Checkpoint` type can be used to progressively
	/// modify and simulate the payload under construction. Checkpoints are
	/// convertible to the final built payload at any time.
	///
	/// There may be multiple versions of the payload under construction, each
	/// call to this function creates a new fork of the payload building process.
	pub fn start(&self) -> Checkpoint<P> {
		Checkpoint::new_at_block(self.clone())
	}
}

struct BlockContextInner<P: Platform> {
	/// The parent block header of the block for which the payload is
	/// being built.
	parent: SealedHeader<types::Header<P>>,

	/// The payload attributes we've got from the CL client
	/// during the ForkchoiceUpdated request.
	attribs: types::PayloadBuilderAttributes<P>,

	/// Access to the state of the environment rooted at the end of the parent
	/// block for which the payload is being built. This state will be read when
	/// there is an attempt to read a storage element that is not present in any
	/// of the checkpoints created on top of this block context.
	///
	/// This state has no changes made to it during the payload building process
	/// through any of the created checkpoints.
	base_state: StateProviderBox,
}
