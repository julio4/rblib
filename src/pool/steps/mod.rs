use {super::*, reth_payload_builder::PayloadId};

mod many;
mod one;

pub use {many::AppendManyOrders, one::AppendOneOrder};

// events emitted by order pool steps

/// Event emitted when an order was considered for inclusion in a payload
#[derive(Debug, Clone)]
pub struct OrderInclusionAttempt(pub B256, pub PayloadId);

/// Event emitted when an order was successfully included in a payload.
#[derive(Debug, Clone)]
pub struct OrderInclusionSuccess(pub B256, pub PayloadId);

/// Event emitted when an order was proposed by the pool but it failed to create
/// a valid checkpoint.
#[derive(Debug, Clone)]
pub struct OrderInclusionFailure<P: Platform>(
	pub B256,
	pub Arc<ExecutionError<P>>,
	pub PayloadId,
);
