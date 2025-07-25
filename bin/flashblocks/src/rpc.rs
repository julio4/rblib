use {
	crate::bundle::{BundleResult, FlashBlocksBundle},
	jsonrpsee::{
		core::{RpcResult, async_trait},
		proc_macros::rpc,
		tracing::debug,
	},
};

pub struct BundleRpcApi;

#[rpc(server, client, namespace = "eth")]
pub trait BundlesRpcApi {
	#[method(name = "sendBundle")]
	async fn send_bundle(&self, tx: FlashBlocksBundle)
	-> RpcResult<BundleResult>;
}

#[async_trait]
impl BundlesRpcApiServer for BundleRpcApi {
	async fn send_bundle(
		&self,
		bundle: FlashBlocksBundle,
	) -> RpcResult<BundleResult> {
		let bundle_hash = bundle.hash();
		debug!("Received eth_sendBundle request over RPC: {bundle:?}");
		Ok(BundleResult { bundle_hash })
	}
}
