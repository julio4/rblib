use {
	crate::{
		Optimism,
		Pipeline,
		pipelines::tests::{FundedAccounts, ONE_ETH},
	},
	alloy::{
		primitives::U256,
		providers::{Identity, ProviderBuilder, RootProvider},
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
	op_alloy::network::Optimism as OpNetwork,
	reth::{
		args::{DatadirArgs, NetworkArgs, RpcServerArgs},
		builder::{NodeBuilder, NodeConfig},
		chainspec::EthChainSpec,
		core::exit::NodeExitFuture,
		tasks::TaskManager,
	},
	reth_optimism_chainspec::OpChainSpec,
	reth_optimism_node::{OpAddOns, OpNode, args::RollupArgs},
	std::sync::{Arc, LazyLock},
	tokio::sync::oneshot,
};

pub struct TestOptimismNode {
	exit_future: NodeExitFuture,
	task_manager: Option<TaskManager>,
	config: NodeConfig<OpChainSpec>,
	provider: RootProvider<OpNetwork>,
	_node_handle: Box<dyn Any + Send>, // keeps reth alive
}

impl TestOptimismNode {
	const MIN_BLOCK_TIME: Duration = Duration::from_secs(1);

	pub async fn new(pipeline: Pipeline<Optimism>) -> eyre::Result<Self> {
		let task_manager = task_manager();
		let config = default_node_config();
		let (rpc_ready_tx, rpc_ready_rx) = oneshot::channel::<()>();

		let node_handle = NodeBuilder::<_, OpChainSpec>::new(config.clone())
			.testing_node(task_manager.executor())
			.with_types::<OpNode>()
			.with_components(OpNode::new(RollupArgs::default()).components())
			.with_add_ons(OpAddOns::default())
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

		let provider = ProviderBuilder::<Identity, Identity, OpNetwork>::default()
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

	pub fn provider(&self) -> &RootProvider<OpNetwork> {
		&self.provider
	}
}

impl Drop for TestOptimismNode {
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

impl Future for TestOptimismNode {
	type Output = eyre::Result<()>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.get_mut().exit_future.poll_unpin(cx)
	}
}

fn task_manager() -> TaskManager {
	TaskManager::new(tokio::runtime::Handle::current())
}

pub fn default_node_config() -> NodeConfig<OpChainSpec> {
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

	NodeConfig::<OpChainSpec>::new(chain_spec())
		.with_datadir_args(datadir)
		.with_rpc(rpc)
		.with_network(network)
}

fn chain_spec() -> Arc<OpChainSpec> {
	static CHAIN_SPEC: LazyLock<Arc<OpChainSpec>> = LazyLock::new(|| {
		let funded_accounts = FundedAccounts::addresses().map(|address| {
			let account =
				GenesisAccount::default().with_balance(U256::from(100 * ONE_ETH));
			(address, account)
		});
		let genesis = include_str!("./artifacts/genesis.json.tmpl");
		let genesis: Genesis =
			serde_json::from_str(genesis).expect("invalid genesis JSON");
		let genesis = genesis.extend_accounts(funded_accounts);
		let chain_spec = OpChainSpec::from_genesis(genesis);
		Arc::new(chain_spec)
	});

	CHAIN_SPEC.clone()
}
