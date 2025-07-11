use {
	crate::{
		pipelines::tests::{
			DEFAULT_BLOCK_GAS_LIMIT,
			ONE_ETH,
			Signer,
			TransactionBuilder,
			framework::FUNDED_PRIVATE_KEYS,
		},
		*,
	},
	alloy::{
		eips::{BlockNumberOrTag, eip7685::RequestsOrHash},
		primitives::{B256, U256},
		providers::{Identity, Provider, ProviderBuilder, RootProvider},
		rpc::types::{Block, Transaction},
	},
	alloy_genesis::GenesisAccount,
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
		chainspec::{ChainSpec, DEV, EthChainSpec, MAINNET},
		core::exit::NodeExitFuture,
		rpc::types::engine::ForkchoiceState,
		tasks::TaskManager,
	},
	reth_ethereum::node::{
		EthEngineTypes,
		EthereumNode,
		engine::EthPayloadAttributes,
		node::EthereumAddOns,
	},
	reth_rpc_api::EngineApiClient,
	std::{
		sync::Arc,
		time::{SystemTime, UNIX_EPOCH},
	},
	tokio::sync::oneshot,
};

pub struct LocalNode {
	exit_future: NodeExitFuture,
	task_manager: Option<TaskManager>,
	config: NodeConfig<ChainSpec>,
	provider: RootProvider,
	_node_handle: Box<dyn Any + Send>, // keeps reth alive
}

impl LocalNode {
	const MIN_BLOCK_TIME: Duration = Duration::from_secs(1);

	pub async fn ethereum(
		pipeline: Pipeline<EthereumMainnet>,
	) -> eyre::Result<Self> {
		let task_manager = task_manager();
		let config = default_node_config();
		let (rpc_ready_tx, rpc_ready_rx) = oneshot::channel::<()>();

		let node_handle = NodeBuilder::new(config.clone())
			.testing_node(task_manager.executor())
			.with_types::<EthereumNode>()
			.with_components(
				EthereumNode::components().payload(pipeline.into_service()),
			)
			.with_add_ons(EthereumAddOns::default())
			.on_rpc_started(move |_, _| {
				let _ = rpc_ready_tx.send(());
				Ok(())
			})
			.launch();

		let node_handle = Box::pin(node_handle).await?;

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
			_node_handle: node_handle,
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

	pub async fn build_new_block(&self) -> eyre::Result<Block<Transaction>> {
		self
			.build_new_block_with_deadline(Duration::from_secs(2))
			.await
	}

	pub async fn build_new_block_with_deadline(
		&self,
		deadline: Duration,
	) -> eyre::Result<Block<Transaction>> {
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

		let payload_attributes = EthPayloadAttributes {
			timestamp: block_timestamp.as_secs(),
			withdrawals: Some(vec![]),
			parent_beacon_block_root: Some(B256::ZERO),
			..Default::default()
		};

		// Start the production of a new block
		let fcu_result = EngineApiClient::<EthEngineTypes>::fork_choice_updated_v3(
			&ipc_client,
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
			&ipc_client,
			payload_id,
		)
		.await?;

		let payload = getpayload_result.execution_payload.clone();
		let new_block_hash = payload.payload_inner.payload_inner.block_hash;

		// Give the newly built payload to the EL node and let it validate it.
		let new_payload_result = EngineApiClient::<EthEngineTypes>::new_payload_v4(
			&ipc_client,
			payload,
			vec![],
			B256::ZERO,
			RequestsOrHash::default(),
		)
		.await?;

		if new_payload_result.is_invalid() {
			return Err(eyre::eyre!(
				"Failed to set new payload: {new_payload_result:#?}"
			));
		}

		// update the canonical chain with the new block without triggering new
		// payload production
		let fcu_result = EngineApiClient::<EthEngineTypes>::fork_choice_updated_v3(
			&ipc_client,
			ForkchoiceState {
				head_block_hash: new_block_hash,
				safe_block_hash: latest_block.header.hash,
				finalized_block_hash: latest_block.header.hash,
			},
			None,
		)
		.await?;

		if fcu_result.is_invalid() {
			return Err(eyre::eyre!("Forkchoice update failed: {fcu_result:#?}"));
		}

		let block = self
			.provider()
			.get_block_by_number(BlockNumberOrTag::Latest)
			.full()
			.await?
			.expect("New block should exist");

		assert_eq!(
			block.header.hash, new_block_hash,
			"New block hash should match the one returned by the payload"
		);

		Ok(block)
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
	let data_path = tempdir.join(format!("rblib.{random_id}.datadir"));

	std::fs::create_dir_all(&data_path)
		.expect("Failed to create temporary data directory");

	let rpc_ipc_path = tempdir.join(format!("rblib.{random_id}.rpc-ipc"));
	let auth_ipc_path = tempdir.join(format!("rblib.{random_id}.auth-ipc"));

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

	NodeConfig::new(Arc::new(chainspec))
		.with_datadir_args(datadir)
		.with_rpc(rpc)
		.with_network(network)
}

fn task_manager() -> TaskManager {
	TaskManager::new(tokio::runtime::Handle::current())
}
