use {
	super::OrderPool,
	crate::{alloy, prelude::*},
	alloy::primitives::B256,
	jsonrpsee::{
		core::{RpcResult, async_trait},
		proc_macros::rpc,
		tracing::debug,
	},
	serde::{Deserialize, Serialize},
};

pub(super) struct BundleRpcApi<P: Platform> {
	pool: OrderPool<P>,
}

impl<P: Platform> BundleRpcApi<P> {
	pub fn new(pool: &OrderPool<P>) -> Self {
		Self { pool: pool.clone() }
	}
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BundleResult {
	#[serde(rename = "bundleHash")]
	pub bundle_hash: B256,
}

#[rpc(server, client, namespace = "eth")]
pub trait BundlesApi<P: Platform> {
	#[method(name = "sendBundle")]
	async fn send_bundle(
		&self,
		bundle: types::Bundle<P>,
	) -> RpcResult<BundleResult>;
}

#[async_trait]
impl<P: Platform> BundlesApiServer<P> for BundleRpcApi<P> {
	async fn send_bundle(
		&self,
		bundle: types::Bundle<P>,
	) -> RpcResult<BundleResult> {
		let bundle_hash = bundle.hash();
		debug!("eth_sendBundle received: {bundle:?}");
		Ok(BundleResult { bundle_hash })
	}
}
