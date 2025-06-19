use {
	crate::Pipeline,
	alloy::providers::{Identity, ProviderBuilder, RootProvider},
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
		chainspec::ChainSpec,
		core::exit::NodeExitFuture,
		tasks::TaskManager,
	},
	reth_ethereum::node::{node::EthereumAddOns, EthEngineTypes, EthereumNode},
	reth_rpc_api::{EngineApiClient, EthApiClient},
	std::sync::Arc,
	tokio::sync::oneshot,
};

pub struct LocalNode {
	exit_future: NodeExitFuture,
	node_handle: Box<dyn Any + Send>,
	task_manager: Option<TaskManager>,
	config: NodeConfig<ChainSpec>,
}

impl LocalNode {
	pub async fn new(pipeline: Pipeline) -> eyre::Result<Self> {
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
			.launch()
			.await?;

		let exit_future = node_handle.node_exit_future;
		let boxed_handle = Box::new(node_handle.node);
		let node_handle: Box<dyn Any + Send> = boxed_handle;

		// Wait for the RPC server to be ready before returning
		rpc_ready_rx.await.expect("Failed to receive ready signal");

		Ok(Self {
			config,
			exit_future,
			node_handle,
			task_manager: Some(task_manager),
		})
	}

	pub async fn provider(&self) -> eyre::Result<RootProvider> {
		Ok(
			ProviderBuilder::<Identity, Identity>::default()
				.connect_ipc(self.config.rpc.ipcpath.clone().into())
				.await?,
		)
	}

	// pub fn engine_api(
	// 	&self,
	// ) -> impl EngineApiClient<EthEngineTypes> + Send + Sync + 'static {
	// 	//&self.config.rpc.auth_ipc_path
	// 	todo!()
	// }
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

	NodeConfig::new(Arc::new(ChainSpec::default()))
		.with_datadir_args(datadir)
		.with_rpc(rpc)
		.with_network(network)
}

fn task_manager() -> TaskManager {
	TaskManager::new(tokio::runtime::Handle::current())
}
