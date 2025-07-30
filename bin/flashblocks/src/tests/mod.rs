use {
	crate::{args::OpRbuilderArgs, build_pipeline, platform::FlashBlocks},
	rblib::{
		alloy::{
			network::{TransactionBuilder, TransactionResponse, TxSignerSync},
			optimism::{
				consensus::{DEPOSIT_TX_TYPE_ID, OpTxEnvelope},
				rpc_types::OpTransactionRequest,
			},
			primitives::{Address, U256},
			signers::local::PrivateKeySigner,
		},
		pool::*,
		prelude::*,
		reth::{
			core::primitives::SignedTransaction,
			optimism::{
				chainspec,
				node::{OpEngineApiBuilder, OpEngineValidatorBuilder, OpNode},
				primitives::OpTransactionSigned,
			},
			primitives::Recovered,
		},
		test_utils::*,
	},
};

mod bundles;
mod ordering;
mod revert;
mod smoke;

impl FlashBlocks {
	pub async fn test_node_with_cli_args(
		cli_args: OpRbuilderArgs,
	) -> eyre::Result<LocalNode<FlashBlocks, OptimismConsensusDriver>> {
		FlashBlocks::create_test_node_with_args(Pipeline::default(), cli_args).await
	}

	pub async fn test_node()
	-> eyre::Result<LocalNode<FlashBlocks, OptimismConsensusDriver>> {
		FlashBlocks::test_node_with_cli_args(OpRbuilderArgs::default()).await
	}

	pub async fn test_node_with_builder_signer()
	-> eyre::Result<LocalNode<FlashBlocks, OptimismConsensusDriver>> {
		FlashBlocks::test_node_with_cli_args(OpRbuilderArgs {
			builder_signer: Some(FundedAccounts::signer(0).into()),
			..Default::default()
		})
		.await
	}
}

impl TestNodeFactory<FlashBlocks> for FlashBlocks {
	type CliExtArgs = OpRbuilderArgs;
	type ConsensusDriver = OptimismConsensusDriver;

	/// Notes:
	///
	/// - Here we are ignoring the `pipeline` argument because we are not
	///   interested in running arbitrary pipelines for this platform, instead we
	///   construct the pipeline based on the CLI arguments.
	async fn create_test_node_with_args(
		_: Pipeline<FlashBlocks>,
		cli_args: Self::CliExtArgs,
	) -> eyre::Result<LocalNode<FlashBlocks, Self::ConsensusDriver>> {
		let chainspec = chainspec::OP_DEV.as_ref().clone().with_funded_accounts();
		LocalNode::new(OptimismConsensusDriver, chainspec, move |builder| {
			let pool = OrderPool::<FlashBlocks>::default();
			let pipeline = build_pipeline(&cli_args, &pool);
			let opnode = OpNode::new(cli_args.rollup_args.clone());

			builder
				.with_types::<OpNode>()
				.with_components(
					opnode
						.components()
						.replace_pool(&pool)
						.payload(pipeline.into_service()),
				)
				.with_add_ons(opnode
						.add_ons_builder::<types::RpcTypes<FlashBlocks>>()
						.build::<_, OpEngineValidatorBuilder, OpEngineApiBuilder<OpEngineValidatorBuilder>>())
				.extend_rpc_modules(move |mut rpc_ctx| pool.configure_rpc(&mut rpc_ctx))
		})
		.await
	}
}

pub fn transfer_tx(
	signer: &PrivateKeySigner,
	nonce: u64,
	value: U256,
) -> Recovered<OpTxEnvelope> {
	let mut tx = OpTransactionRequest::default()
		.with_nonce(nonce)
		.with_to(Address::random())
		.value(value)
		.with_gas_price(1_000_000_000)
		.with_gas_limit(21_000)
		.with_max_priority_fee_per_gas(1_000_000)
		.with_max_fee_per_gas(2_000_000)
		.build_unsigned()
		.expect("valid transaction request");

	let sig = signer
		.sign_transaction_sync(&mut tx)
		.expect("signing should succeed");

	OpTransactionSigned::new_unhashed(tx, sig) //
		.with_signer(signer.address())
}

pub fn transfer_tx_compact(
	signer: u32,
	nonce: u64,
	value: u64,
) -> Recovered<OpTxEnvelope> {
	let signer = FundedAccounts::signer(signer);
	transfer_tx(&signer, nonce, U256::from(value))
}

#[tokio::test]
async fn testing_node_works_with_flashblocks_platform() -> eyre::Result<()> {
	let node = FlashBlocks::test_node().await?;
	assert_eq!(node.chain_id(), 1337);

	let block = node.next_block().await?;

	assert_eq!(block.header.number, 1);
	assert_eq!(block.tx_count(), 1); // sequencer deposit tx
	assert_eq!(
		block.tx(0).unwrap().transaction_type().unwrap(),
		DEPOSIT_TX_TYPE_ID,
		"Optimism sequencer transaction should be a deposit tx"
	);
	Ok(())
}
