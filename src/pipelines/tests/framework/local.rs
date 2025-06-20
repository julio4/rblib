use {
	crate::{
		pipelines::tests::{
			framework::FUNDED_PRIVATE_KEYS,
			Signer,
			TransactionBuilder,
			DEFAULT_BLOCK_GAS_LIMIT,
			ONE_ETH,
		},
		*,
	},
	alloy::{
		eips::BlockNumberOrTag,
		primitives::{B256, U256},
		providers::{Identity, Provider, ProviderBuilder, RootProvider},
	},
	alloy_genesis::{Genesis, GenesisAccount},
	core::{
		any::Any,
		pin::Pin,
		task::{Context, Poll},
		time::Duration,
	},
	futures::FutureExt,
	nanoid::nanoid,
	reth::{
		args::{DatadirArgs, NetworkArgs, RpcServerArgs},
		builder::{NodeBuilder, NodeConfig},
		chainspec::{ChainSpec, DEV, MAINNET},
		core::exit::NodeExitFuture,
		rpc::types::{engine::ForkchoiceState, Block},
		tasks::TaskManager,
	},
	reth_ethereum::node::{
		engine::EthPayloadAttributes,
		node::EthereumAddOns,
		EthEngineTypes,
		EthereumNode,
	},
	reth_rpc_api::{EngineApiClient, EthApiClient},
	std::{
		sync::Arc,
		time::{Instant, SystemTime, UNIX_EPOCH},
	},
	tokio::sync::oneshot,
	tracing::info,
};

pub struct LocalNode {
	exit_future: NodeExitFuture,
	node_handle: Box<dyn Any + Send>,
	task_manager: Option<TaskManager>,
	config: NodeConfig<ChainSpec>,
	provider: RootProvider,
}

impl LocalNode {
	const MIN_BLOCK_TIME: Duration = Duration::from_secs(1);

	pub async fn ethereum(pipeline: Pipeline) -> eyre::Result<Self> {
		let task_manager = task_manager();
		let config = default_node_config();
		let (rpc_ready_tx, rpc_ready_rx) = oneshot::channel::<()>();
		let node_handle = NodeBuilder::new(config.clone())
			.testing_node(task_manager.executor())
			.with_types::<EthereumNode>()
			.with_components(
				EthereumNode::components()
					.payload(pipeline.into_service::<EthereumMainnet, _, _, _>()),
			)
			.with_add_ons(EthereumAddOns::default())
			.on_rpc_started(move |_, _| {
				let _ = rpc_ready_tx.send(());
				Ok(())
			})
			.launch()
			.await?;

		let exit_future = node_handle.node_exit_future;
		let boxed_handle = Box::new(node_handle.node);
		let node_handle: Box<dyn Any + Send> = boxed_handle;

		// Wait for the RPC server to be ready before returning
		rpc_ready_rx.await.expect("Failed to receive ready signal");

		let provider = ProviderBuilder::<Identity, Identity>::default()
			.connect_ipc(config.rpc.ipcpath.clone().into())
			.await?;

		Ok(Self {
			config,
			exit_future,
			node_handle,
			task_manager: Some(task_manager),
			provider,
		})
	}

	pub fn chain_id(&self) -> u64 {
		self.config.chain.chain_id()
	}

	pub fn provider(&self) -> &RootProvider {
		&self.provider
	}

	pub fn new_transaction(&self) -> TransactionBuilder {
		TransactionBuilder::new(self.provider().clone())
			.with_chain_id(self.chain_id())
	}

	pub async fn build_new_block(
		&self,
	) -> eyre::Result<types::Block<EthereumMainnet>> {
		self
			.build_new_block_with_deadline(Duration::from_secs(2))
			.await
	}

	pub async fn build_new_block_with_deadline(
		&self,
		deadline: Duration,
	) -> eyre::Result<types::Block<EthereumMainnet>> {
		let ipc_path = self.config.rpc.auth_ipc_path.clone();
		let ipc_client = reth_ipc::client::IpcClientBuilder::default()
			.build(&ipc_path)
			.await
			.expect("Failed to create ipc client");

		let latest_block = self
			.provider()
			.get_block_by_number(BlockNumberOrTag::Latest)
			.await?
			.expect("Latest block should exist");

		let latest_timestamp = Duration::from_secs(latest_block.header.timestamp);

		// calculate the timestamp for the new block
		let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?;
		let elapsed_time = current_timestamp - latest_timestamp;
		let block_deadline = deadline.max(Self::MIN_BLOCK_TIME);
		let block_timestamp = latest_timestamp + block_deadline + elapsed_time;

		info!(
			"Building new block with timestamp: {}",
			block_timestamp.as_secs()
		);
		let payload_attributes = EthPayloadAttributes {
			timestamp: block_timestamp.as_secs(),
			withdrawals: Some(vec![]),
			parent_beacon_block_root: Some(B256::ZERO),
			..Default::default()
		};

		// Start the production of a new block
		let fcu_result = EngineApiClient::<EthEngineTypes>::fork_choice_updated_v3(
			(&ipc_client).into(),
			ForkchoiceState {
				head_block_hash: latest_block.header.hash,
				safe_block_hash: latest_block.header.hash,
				finalized_block_hash: latest_block.header.hash,
			},
			Some(payload_attributes),
		)
		.await?;

		if fcu_result.is_invalid() {
			return Err(eyre::eyre!("Forkchoice update failed: {fcu_result:#?}"));
		}

		let payload_id = fcu_result.payload_id.expect(
			"validated that it is a valid result and should have a payload ID",
		);

		// give the node some time to produce the block
		tokio::time::sleep(deadline).await;

		// Retrieve the payload using the payload ID
		let getpayload_result = EngineApiClient::<EthEngineTypes>::get_payload_v4(
			(&ipc_client).into(),
			payload_id,
		)
		.await?;

		info!("Payload ID: {payload_id}, Payload: {getpayload_result:#?}");

		// set the newly produced payload as the next block

		todo!()
	}
}

impl Drop for LocalNode {
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

impl Future for LocalNode {
	type Output = eyre::Result<()>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.get_mut().exit_future.poll_unpin(cx)
	}
}

pub fn default_node_config() -> NodeConfig<ChainSpec> {
	let tempdir = std::env::temp_dir();
	let random_id = nanoid!();

	let data_path = tempdir
		.join(format!("rblib.{random_id}.datadir"))
		.to_path_buf();

	std::fs::create_dir_all(&data_path)
		.expect("Failed to create temporary data directory");

	let rpc_ipc_path = tempdir
		.join(format!("rblib.{random_id}.rpc-ipc"))
		.to_path_buf();

	let auth_ipc_path = tempdir
		.join(format!("rblib.{random_id}.auth-ipc"))
		.to_path_buf();

	let mut rpc = RpcServerArgs::default().with_auth_ipc();
	rpc.ws = false;
	rpc.http = false;
	rpc.auth_port = 0;
	rpc.ipcpath = rpc_ipc_path.to_string_lossy().into();
	rpc.auth_ipc_path = auth_ipc_path.to_string_lossy().into();

	let mut network = NetworkArgs::default().with_unused_ports();
	network.discovery.disable_discovery = true;

	let datadir = DatadirArgs {
		datadir: data_path
			.to_string_lossy()
			.parse()
			.expect("Failed to parse data dir path"),
		static_files_path: None,
	};

	let prefunded_account = Signer::try_from_secret(
		FUNDED_PRIVATE_KEYS[0]
			.parse()
			.expect("Invalid hardcoded private key"),
	)
	.expect("Failed to create signer from hardcoded private key");

	let funded_accounts = FUNDED_PRIVATE_KEYS.iter().map(|prv| {
		let address = Signer::try_from_secret(
			prv.parse().expect("Invalid hardcoded private key"),
		)
		.expect("Failed to create signer from hardcoded private key")
		.address;
		let account =
			GenesisAccount::default().with_balance(U256::from(100 * ONE_ETH));
		(address, account)
	});

	let mut chainspec = DEV.as_ref().clone();
	chainspec.genesis = chainspec
		.genesis
		.extend_accounts(funded_accounts)
		.with_gas_limit(DEFAULT_BLOCK_GAS_LIMIT);
	chainspec.hardforks = MAINNET.hardforks.clone();

	// .genesis
	// .clone()

	// .with_timestamp(
	// 	SystemTime::now()
	// 		.duration_since(UNIX_EPOCH)
	// 		.unwrap()
	// 		.as_secs(),
	// );

	// let mut chainspec = ChainSpec::from_genesis(genesis);
	// chainspec.hardforks = MAINNET.hardforks.clone();

	NodeConfig::new(Arc::new(chainspec))
		.with_datadir_args(datadir)
		.with_rpc(rpc)
		.with_network(network)
}

fn task_manager() -> TaskManager {
	TaskManager::new(tokio::runtime::Handle::current())
}
