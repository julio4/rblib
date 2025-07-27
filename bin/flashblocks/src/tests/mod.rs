use {
	crate::{args::OpRbuilderArgs, platform::FlashBlocks},
	rblib::{
		alloy,
		alloy::{
			network::{TransactionBuilder, TransactionResponse, TxSignerSync},
			optimism::{
				consensus::{DEPOSIT_TX_TYPE_ID, OpTxEnvelope},
				rpc_types::OpTransactionRequest,
			},
			primitives::{Address, U256},
			signers::local::PrivateKeySigner,
		},
		pool::{AppendOneOrder, OrderPool},
		prelude::*,
		reth::{
			core::primitives::SignedTransaction,
			optimism::{
				chainspec,
				node::{OpAddOns, OpNode},
				primitives::OpTransactionSigned,
			},
			primitives::Recovered,
		},
		steps::*,
		test_utils::*,
	},
};

mod bundles;

impl NetworkSelector for FlashBlocks {
	type Network = alloy::optimism::network::Optimism;
}

impl TestNodeFactory<FlashBlocks> for FlashBlocks {
	type ConsensusDriver = OptimismConsensusDriver;

	async fn create_test_node(
		_: Pipeline<FlashBlocks>,
	) -> eyre::Result<LocalNode<FlashBlocks, Self::ConsensusDriver>> {
		let chainspec = chainspec::OP_DEV.as_ref().clone().with_funded_accounts();
		LocalNode::new(OptimismConsensusDriver, chainspec, move |builder| {
			let cli_args = OpRbuilderArgs::default();
			let pool = OrderPool::<FlashBlocks>::default();

			let pipeline = Pipeline::<FlashBlocks>::default()
				.with_prologue(OptimismPrologue)
				.with_pipeline(
					Loop,
					(
						AppendOneOrder::from_pool(&pool),
						OrderByPriorityFee,
						RemoveRevertedTransactions,
					),
				)
				.with_epilogue(BuilderEpilogue);

			let opnode = OpNode::new(cli_args.rollup_args.clone());

			builder
				.with_types::<OpNode>()
				.with_components(opnode.components().payload(pipeline.into_service()))
				.with_add_ons(OpAddOns::default())
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
async fn test_node_produces_empty_block() -> eyre::Result<()> {
	let node = FlashBlocks::create_test_node(Pipeline::default()).await?;
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
