use {
	crate::*,
	alloc::sync::Arc,
	reth::{
		api::{ConfigureEvm, PayloadBuilderAttributes, PayloadBuilderError},
		payload::PayloadId,
		primitives::SealedHeader,
		providers::{StateProvider, StateProviderBox},
		revm::{database::StateProviderDatabase, State},
	},
	reth_evm::{block::BlockExecutionError, execute::BlockBuilder},
	thiserror::Error,
};

#[derive(Debug, Error)]
pub enum Error<P: Platform> {
	#[error("Evm execution error: {0}")]
	Evm(types::EvmError<P>),

	#[error("State error: {0}")]
	PayloadBuilder(#[from] PayloadBuilderError),

	#[error("Block execution error: {0}")]
	BlockExecution(#[from] BlockExecutionError),
}

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
		evm_config: P::EvmConfig,
		chainspec: &P::ChainSpec,
	) -> Result<Self, Error<P>> {
		let next_block_env = P::next_block_environment_context(
			//
			chainspec,
			parent.header(),
			&attribs,
		);

		let evm_env = evm_config //
			.next_evm_env(&parent, &next_block_env)
			.map_err(Error::Evm)?;

		let mut base_state = State::builder()
			.with_database(StateProviderDatabase(base_state))
			.with_bundle_update()
			.build();

		// prepare the base state for the next block
		evm_config
			.builder_for_next_block(&mut base_state, &parent, next_block_env)
			.map_err(Error::Evm)?
			.apply_pre_execution_changes()?;

		Ok(Self {
			inner: Arc::new(BlockContextInner {
				evm_env,
				parent,
				attribs,
				base_state,
				evm_config,
			}),
		})
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
	pub fn base_state(&self) -> &dyn StateProvider {
		self.inner.base_state.database.as_ref()
	}

	/// Returns the EVM configuration that is used to create EVM instances
	/// for executing transactions in the payload under construction.
	pub fn evm_config(&self) -> &P::EvmConfig {
		&self.inner.evm_config
	}

	/// Returns a evm environment preconfigured for executing transactions in
	/// the payload under construction.
	pub fn evm_env(&self) -> &types::EvmEnv<P> {
		&self.inner.evm_env
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

	/// Context object for constucting evm instance for the block that is being
	/// built. We use this context to apply any chain and fork specific state
	/// updates that are defined in the chainspec.
	evm_env: types::EvmEnv<P>,

	/// Access to the state of the environment rooted at the end of the parent
	/// block for which the payload is being built. This state will be read when
	/// there is an attempt to read a storage element that is not present in any
	/// of the checkpoints created on top of this block context.
	///
	/// This state has no changes made to it during the payload building process
	/// through any of the created checkpoints.
	base_state: State<StateProviderDatabase<StateProviderBox>>,

	/// The EVM factory configured for the environment in which we are building
	/// the payload. This type is used to create individual EVM instances that
	/// execute transactions in the payload under construction.
	evm_config: P::EvmConfig,
}
