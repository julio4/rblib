use {
	crate::{
		pipelines::tests::{
			DEFAULT_BLOCK_GAS_LIMIT,
			FundedAccounts,
			NetworkSelector,
			ONE_ETH,
			framework::{TestNodeFactory, node::LocalNode},
		},
		tests::ConsensusDriver,
		*,
	},
	alloy::{
		eips::{BlockNumberOrTag, eip7685::RequestsOrHash},
		primitives::{B256, U256},
		providers::Provider,
		rpc::types::Block,
	},
	alloy_genesis::GenesisAccount,
	nanoid::nanoid,
	reth::{
		args::{DatadirArgs, NetworkArgs, RpcServerArgs},
		builder::NodeConfig,
		chainspec::{ChainSpec, DEV, MAINNET},
		rpc::types::engine::ForkchoiceState,
	},
	reth_ethereum::node::{
		EthEngineTypes,
		EthereumNode,
		engine::EthPayloadAttributes,
		node::EthereumAddOns,
	},
	reth_ipc::client::IpcClientBuilder,
	reth_payload_builder::PayloadId,
	reth_rpc_api::EngineApiClient,
	std::sync::Arc,
};

impl NetworkSelector for Ethereum {
	type Network = alloy::network::Ethereum;
}

impl TestNodeFactory<Ethereum> for Ethereum {
	type ConsensusDriver = EthConsensusDriver;

	async fn create_test_node(
		pipeline: Pipeline<Ethereum>,
	) -> eyre::Result<LocalNode<Ethereum, Self::ConsensusDriver>> {
		LocalNode::new(EthConsensusDriver, default_node_config(), move |builder| {
			builder
				.with_types::<EthereumNode>()
				.with_components(
					EthereumNode::components().payload(pipeline.into_service()),
				)
				.with_add_ons(EthereumAddOns::default())
		})
		.await
	}
}

pub struct EthConsensusDriver;
impl ConsensusDriver<Ethereum> for EthConsensusDriver {
	type Params = ();

	async fn start_building(
		&self,
		node: &LocalNode<Ethereum, Self>,
		target_timestamp: u64,
		(): &Self::Params,
	) -> eyre::Result<PayloadId> {
		let ipc_path = node.config().rpc.auth_ipc_path.clone();
		let ipc_client = IpcClientBuilder::default()
			.build(&ipc_path)
			.await
			.expect("Failed to create ipc client");

		let latest_block = node
			.provider()
			.get_block_by_number(BlockNumberOrTag::Latest)
			.await?
			.expect("Latest block should exist");

		let payload_attributes = EthPayloadAttributes {
			timestamp: target_timestamp,
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

		Ok(fcu_result.payload_id.expect(
			"validated that it is a valid result and should have a payload ID",
		))
	}

	async fn finish_building(
		&self,
		node: &LocalNode<Ethereum, Self>,
		payload_id: PayloadId,
		(): &Self::Params,
	) -> eyre::Result<Block> {
		let ipc_path = node.config().rpc.auth_ipc_path.clone();
		let ipc_client = IpcClientBuilder::default()
			.build(&ipc_path)
			.await
			.expect("Failed to create ipc client");

		let latest_block = node
			.provider()
			.get_block_by_number(BlockNumberOrTag::Latest)
			.await?
			.expect("Latest block should exist");

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

		let block = node
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

	let funded_accounts = FundedAccounts::addresses().map(|address| {
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
