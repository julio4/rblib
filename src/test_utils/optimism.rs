use {
	super::*,
	crate::*,
	alloy::{
		consensus::SignableTransaction,
		eips::{BlockNumberOrTag, Encodable2718, eip7685::Requests},
		hex,
		network::TxSignerSync,
		optimism::{
			consensus::{OpTxEnvelope, OpTypedTransaction, TxDeposit},
			rpc_types::Transaction,
		},
		primitives::{B256, Bytes, TxKind, U256, address},
		providers::Provider,
	},
	alloy_genesis::{Genesis, GenesisAccount},
	reth::{
		ethereum::node::engine::EthPayloadAttributes as PayloadAttributes,
		optimism::{
			chainspec::OpChainSpec,
			node::{OpAddOns, OpEngineTypes, OpNode, OpPayloadAttributes},
		},
		payload::builder::PayloadId,
		rpc::types::{Block, engine::ForkchoiceState},
	},
	reth_ipc::client::IpcClientBuilder,
	reth_optimism_rpc::OpEngineApiClient,
	serde_json::from_str,
};

impl NetworkSelector for Optimism {
	type Network = op_alloy::network::Optimism;
}

impl TestNodeFactory<Optimism> for Optimism {
	type ConsensusDriver = OptimismConsensusDriver;

	async fn create_test_node(
		pipeline: Pipeline<Optimism>,
	) -> eyre::Result<LocalNode<Optimism, Self::ConsensusDriver>> {
		LocalNode::new(OptimismConsensusDriver, chainspec(), move |builder| {
			builder
				.with_types::<OpNode>()
				.with_components(
					OpNode::default()
						.components()
						.payload(pipeline.into_service()),
				)
				.with_add_ons(OpAddOns::default())
		})
		.await
	}
}

const DEFAULT_GAS_LIMIT: u64 = 200_000_000;

pub struct OptimismConsensusDriver;
impl ConsensusDriver<Optimism> for OptimismConsensusDriver {
	type Params = ();

	async fn start_building(
		&self,
		node: &LocalNode<Optimism, Self>,
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

		// Add L1 block info as the first transaction in every L2 block
		// This deposit transaction contains L1 block metadata required by the L2
		// chain Currently using hardcoded data from L1 block 124665056
		// If this info is not provided, Reth cannot decode the receipt for any
		// transaction in the block since it also includes this info as part of
		// the result. It does not matter if the to address
		// (4200000000000000000000000000000000000015) is not deployed on the L2
		// chain since Reth queries the block to get the info and not the contract.
		let block_info_tx: Bytes = {
			let deposit_tx = TxDeposit {
				source_hash: B256::default(),
				from: address!("DeaDDEaDDeAdDeAdDEAdDEaddeAddEAdDEAd0001"),
				to: TxKind::Call(address!("4200000000000000000000000000000000000015")),
				mint: 0,
				value: U256::default(),
				gas_limit: 210_000,
				is_system_transaction: false,
				input: FJORD_DATA.into(),
			};

			let mut tx = OpTypedTransaction::Deposit(deposit_tx);
			let signer = FundedAccounts::random();
			let signature = signer.sign_transaction_sync(&mut tx)?;
			let envelope: OpTxEnvelope = tx.into_signed(signature).into();
			envelope.encoded_2718().into()
		};

		let payload_attributes = OpPayloadAttributes {
			payload_attributes: PayloadAttributes {
				timestamp: target_timestamp,
				parent_beacon_block_root: Some(B256::ZERO),
				withdrawals: Some(vec![]),
				..Default::default()
			},
			transactions: Some(vec![block_info_tx]),
			gas_limit: Some(DEFAULT_GAS_LIMIT),
			..Default::default()
		};

		let fcu_result =
			OpEngineApiClient::<OpEngineTypes>::fork_choice_updated_v3(
				&ipc_client,
				ForkchoiceState {
					head_block_hash: latest_block.header.hash,
					safe_block_hash: latest_block.header.hash,
					finalized_block_hash: latest_block.header.hash,
				},
				Some(payload_attributes),
			)
			.await?;

		if fcu_result.payload_status.is_invalid() {
			return Err(eyre::eyre!("Forkchoice update failed: {fcu_result:?}"));
		}

		fcu_result.payload_id.ok_or_else(|| {
			eyre::eyre!("Forkchoice update did not return a payload ID")
		})
	}

	async fn finish_building(
		&self,
		node: &LocalNode<Optimism, Self>,
		payload_id: PayloadId,
		(): &Self::Params,
	) -> eyre::Result<Block<Transaction>> {
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

		let getpayload_result = OpEngineApiClient::<OpEngineTypes>::get_payload_v4(
			&ipc_client,
			payload_id,
		)
		.await?;

		let payload = getpayload_result.execution_payload;
		let new_block_hash =
			payload.payload_inner.payload_inner.payload_inner.block_hash;

		// Give the newly built payload to the EL node and let it validate it.
		let new_payload_result =
			OpEngineApiClient::<OpEngineTypes>::new_payload_v4(
				&ipc_client,
				payload,
				vec![],
				B256::ZERO,
				Requests::default(),
			)
			.await?;

		if new_payload_result.is_invalid() {
			return Err(eyre::eyre!(
				"Failed to set new payload: {new_payload_result:#?}"
			));
		}

		let fcu_result =
			OpEngineApiClient::<OpEngineTypes>::fork_choice_updated_v3(
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

fn chainspec() -> OpChainSpec {
	let funded_accounts = FundedAccounts::addresses().map(|address| {
		let account =
			GenesisAccount::default().with_balance(U256::from(100 * ONE_ETH));
		(address, account)
	});

	let genesis = include_str!("./artifacts/op-genesis.json");
	let genesis: Genesis = from_str(genesis).expect("invalid genesis JSON");
	let genesis = genesis.extend_accounts(funded_accounts);
	OpChainSpec::from_genesis(genesis)
}

// L1 block info for OP mainnet block 124665056 (stored in input of tx at index
// 0)
//
// https://optimistic.etherscan.io/tx/0x312e290cf36df704a2217b015d6455396830b0ce678b860ebfcc30f41403d7b1
const FJORD_DATA: &[u8] = &hex!(
	"440a5e200000146b000f79c500000000000000040000000066d052e700000000013ad8a
    3000000000000000000000000000000000000000000000000000000003ef12787000000
    00000000000000000000000000000000000000000000000000000000012fdf87b89884a
    61e74b322bbcf60386f543bfae7827725efaaf0ab1de2294a5900000000000000000000
    00006887246668a3b87f54deb3b94ba47a6f63f32985"
);
