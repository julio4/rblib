use {
	super::{NetworkSelector, select, utils::TransactionRequestExt},
	crate::{
		Platform,
		pipelines::tests::FundedAccounts,
		tests::ConsensusDriver,
		types,
	},
	alloy::{
		consensus::{BlockHeader, SignableTransaction},
		eips::{BlockNumberOrTag, Encodable2718},
		network::{BlockResponse, TransactionBuilder, TxSignerSync},
		providers::*,
		signers::Signature,
	},
	core::{
		any::Any,
		pin::Pin,
		task::{Context, Poll},
		time::Duration,
	},
	futures::FutureExt,
	reth::{
		chainspec::EthChainSpec,
		core::exit::NodeExitFuture,
		tasks::TaskManager,
	},
	reth_ethereum::provider::db::{DatabaseEnv, test_utils::TempDatabase},
	reth_node_builder::{rpc::RethRpcAddOns, *},
	std::{
		sync::Arc,
		time::{SystemTime, UNIX_EPOCH},
	},
	tokio::sync::oneshot,
};

/// This is used to create local execution nodes for testing purposes that can
/// be used to test payload building pipelines. This type contains everything to
/// trigger full payload building lifecycle and interact with the node through
/// RPC.
pub struct LocalNode<P, C>
where
	P: Platform + NetworkSelector,
	C: ConsensusDriver<P>,
{
	/// The consensus driver used to build blocks and interact with the node.
	consensus: C,
	/// The exit future that can be used to wait for the node to exit. In
	/// practice this never completes, because the node is kept alive until the
	/// test is done.
	exit_future: NodeExitFuture,
	/// The configuration of the node.
	config: NodeConfig<types::ChainSpec<P>>,
	/// The provider used to interact with the node.
	provider: RootProvider<select::Network<P>>,
	/// The task manager used to manage tasks in the node.
	task_manager: Option<TaskManager>,
	/// The block time used by the node.
	block_time: Duration,
	/// Keeps reth alive, this is used to ensure that the node does not exit
	/// while we are still using it.
	_node_handle: Box<dyn Any + Send>,
}

impl<P, C> LocalNode<P, C>
where
	P: Platform + NetworkSelector,
	C: ConsensusDriver<P>,
{
	/// Unless otherwise specified, The payload building process will be given
	/// this amount of time to return a payload. Also blocks cannot have lower
	/// block times than this because the timestamp resolution is in seconds.
	const MIN_BLOCK_TIME: Duration = Duration::from_secs(1);

	/// Creates a new local node instance with the given consensus driver and
	/// reth node configuration specific to the platform and network.
	pub async fn new<NodeBuilderFn, T, CB, AO>(
		consensus: C,
		config: NodeConfig<types::ChainSpec<P>>,
		build_node: NodeBuilderFn,
	) -> eyre::Result<Self>
	where
		NodeBuilderFn:
			FnOnce(
				WithLaunchContext<
					NodeBuilder<Arc<TempDatabase<DatabaseEnv>>, types::ChainSpec<P>>,
				>,
			) -> WithLaunchContext<NodeBuilderWithComponents<T, CB, AO>>,
		T: FullNodeTypes,
		CB: NodeComponentsBuilder<T>,
		AO: RethRpcAddOns<NodeAdapter<T, CB::Components>> + 'static,
		EngineNodeLauncher: LaunchNode<
				NodeBuilderWithComponents<T, CB, AO>,
				Node = NodeHandle<NodeAdapter<T, CB::Components>, AO>,
			>,
	{
		let task_manager = TaskManager::new(tokio::runtime::Handle::current());
		let task_executor = task_manager.executor();
		let (rpc_ready_tx, rpc_ready_rx) = oneshot::channel::<()>();

		let node_builder = build_node(
			NodeBuilder::new(config.clone()).testing_node(task_executor.clone()),
		)
		.on_rpc_started(move |_, _| {
			let _ = rpc_ready_tx.send(());
			Ok(())
		});

		let node_handle = node_builder.launch();
		let node_handle = Box::pin(node_handle).await?;

		let exit_future = node_handle.node_exit_future;
		let boxed_handle = Box::new(node_handle.node);
		let node_handle: Box<dyn Any + Send> = boxed_handle;

		// Wait for the RPC server to be ready before returning
		rpc_ready_rx.await?;

		let provider =
			ProviderBuilder::<Identity, Identity, select::Network<P>>::default()
				.connect_ipc(config.rpc.ipcpath.clone().into())
				.await?;

		Ok(Self {
			consensus,
			exit_future,
			config,
			provider,
			block_time: Self::MIN_BLOCK_TIME,
			task_manager: Some(task_manager),
			_node_handle: node_handle,
		})
	}

	/// Sets the time interval payload builders are given to build a new payload.
	/// This is the interval between forkchoiceupdate engine api call and
	/// getpayload call from the CL node.
	pub fn set_block_time(&mut self, block_time: Duration) {
		self.block_time = block_time.max(Self::MIN_BLOCK_TIME);
	}

	/// Returns the node configuration used to create this local node.
	pub const fn config(&self) -> &NodeConfig<types::ChainSpec<P>> {
		&self.config
	}

	/// Gets the chain ID as defined in the test Genesis block config.
	pub fn chain_id(&self) -> u64 {
		self.config().chain.chain_id()
	}

	/// Returns a provider connected to this local node instance.
	pub const fn provider(&self) -> &RootProvider<select::Network<P>> {
		&self.provider
	}

	/// Returns a reference to the node's consensus driver.
	/// In most cases you will not want to use this method directly, but rather
	/// use the `next_block` method to trigger the building of a new block.
	pub const fn consensus(&self) -> &C {
		&self.consensus
	}

	/// Triggers the building of a new block on this node with default paramters
	/// and returns the newly built block.
	pub async fn next_block(&self) -> eyre::Result<select::BlockResponse<P>> {
		self.next_block_with_params(C::Params::default()).await
	}

	/// Triggers the building of a new block on this node with user provided
	/// paramters and returns the newly built block.
	pub async fn next_block_with_params(
		&self,
		params: C::Params,
	) -> eyre::Result<select::BlockResponse<P>> {
		let latest_block = self
			.provider()
			.get_block_by_number(BlockNumberOrTag::Latest)
			.await?
			.ok_or_else(|| eyre::eyre!("Failed to get latest block from the node"))?;

		let latest_timestamp =
			Duration::from_secs(latest_block.header().timestamp());

		// calculate the timestamp for the new block
		let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?;
		let elapsed_time = current_timestamp - latest_timestamp;
		let target_timestamp = latest_timestamp + self.block_time + elapsed_time;

		// start the payload building process
		let payload_id = self
			.consensus
			.start_building(self, target_timestamp.as_secs(), &params)
			.await?;

		// wait for the payload to be built on the EL node.
		tokio::time::sleep(self.block_time).await;

		let block = self
			.consensus
			.finish_building(self, payload_id, &params)
			.await?;

		Ok(block)
	}

	/// Creates a new transaction builder compatible with this local node.
	pub fn build_tx(&self) -> select::TransactionRequest<P> {
		select::TransactionRequest::<P>::default()
			.with_chain_id(self.chain_id())
			.with_random_funded_signer()
			.with_gas_limit(210_000)
	}

	/// Sends a transaction built using the builder obtained from `build_tx` to
	/// the local node RPC endpoint.
	///
	/// Use this method if the transaction's signer is one of the known funded
	/// accounts. If the transaction's signer is not one of the known funded
	/// accounts, use `sign_and_send_tx` instead and provide the signer
	/// explicitly.
	pub async fn send_tx(
		&self,
		request: impl TransactionBuilder<select::Network<P>>,
	) -> eyre::Result<PendingTransactionBuilder<select::Network<P>>> {
		let Some(from) = request.from() else {
			return Err(eyre::eyre!(
				"Transaction request must have a 'from' field, use sign_and_send_tx \
				 instead"
			));
		};

		let Some(signer) = FundedAccounts::by_address(from) else {
			return Err(eyre::eyre!("No funded account found for address: {}", from));
		};

		self.sign_and_send_tx(request, signer).await
	}

	/// Sends a transaction built using the builder obtained from `build_tx` to
	/// the local node RPC endpoint. This method will sign the transaction
	/// using the provided signer.
	pub async fn sign_and_send_tx(
		&self,
		request: impl TransactionBuilder<select::Network<P>>,
		signer: impl TxSignerSync<Signature> + Send + Sync + 'static,
	) -> eyre::Result<PendingTransactionBuilder<select::Network<P>>> {
		let request = request.with_from(signer.address());

		// if nonce is not explictly set, fetch it from the provider
		let request = match request.nonce() {
			Some(_) => request,
			None => request.with_nonce(
				self
					.provider
					.get_transaction_count(signer.address())
					.pending()
					.await?,
			),
		};

		let request = if let Some(gas_price) = request.gas_price() {
			request.with_gas_price(gas_price)
		} else {
			let gas_price = self.provider().get_gas_price().await?;
			request.with_gas_price(gas_price)
		};

		let request = match (
			request.max_fee_per_gas(),
			request.max_priority_fee_per_gas(),
		) {
			(Some(max_fee), Some(max_priority_fee)) => request
				.with_max_fee_per_gas(max_fee)
				.with_max_priority_fee_per_gas(max_priority_fee),
			(None, Some(_)) => {
				let fee_estimate = self.provider().estimate_eip1559_fees().await?;
				request.with_max_fee_per_gas(fee_estimate.max_fee_per_gas)
			}
			(Some(_), None) => {
				let fee_estimate = self.provider().estimate_eip1559_fees().await?;
				request
					.with_max_priority_fee_per_gas(fee_estimate.max_priority_fee_per_gas)
			}
			(None, None) => {
				let fee_estimate = self.provider().estimate_eip1559_fees().await?;
				request
					.with_max_fee_per_gas(fee_estimate.max_fee_per_gas)
					.with_max_priority_fee_per_gas(fee_estimate.max_priority_fee_per_gas)
			}
		};

		let mut tx = request.build_unsigned()?;
		let signature = signer.sign_transaction_sync(&mut tx)?;
		let envelope: select::TxEnvelope<P> = tx.into_signed(signature).into();
		let encoded = envelope.encoded_2718();

		self
			.provider()
			.send_raw_transaction(&encoded)
			.await
			.map_err(Into::into)
	}
}

impl<P, C> Drop for LocalNode<P, C>
where
	P: Platform + NetworkSelector,
	C: ConsensusDriver<P>,
{
	fn drop(&mut self) {
		if let Some(task_manager) = self.task_manager.take() {
			task_manager.graceful_shutdown_with_timeout(Duration::from_secs(3));

			std::fs::remove_dir_all(self.config.datadir().to_string())
				.unwrap_or_else(|e| {
					panic!(
						"Failed to remove temporary data directory {}: {e}",
						self.config.datadir()
					)
				});
		}
	}
}

impl<P, C> Future for LocalNode<P, C>
where
	P: Platform + NetworkSelector,
	C: ConsensusDriver<P>,
{
	type Output = eyre::Result<()>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.get_mut().exit_future.poll_unpin(cx)
	}
}
