use {
	super::{Order, OrderPool},
	crate::{alloy, prelude::*},
	alloy::primitives::B256,
	jsonrpsee::{
		core::{RpcResult, async_trait},
		proc_macros::rpc,
		tracing::debug,
		types::{ErrorCode, ErrorObject},
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
		if bundle.transactions().is_empty() {
			return Err(ErrorObject::borrowed(
				ErrorCode::InvalidParams.code(),
				"bundle must contain at least one transaction",
				None,
			));
		}

		let bundle_hash = bundle.hash();
		debug!(hash = %bundle_hash, "eth_sendBundle received: {bundle:?}");
		self.pool.insert(Order::Bundle(bundle));
		Ok(BundleResult { bundle_hash })
	}
}
