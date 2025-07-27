//! Order pool

use crate::{
	alloy::primitives::B256,
	prelude::*,
	reth::{ethereum::primitives::SignedTransaction, primitives::Recovered},
};

mod backend;
mod rpc;
mod steps;

// Order Pool public API
pub use {
	backend::OrderPool,
	rpc::{BundleResult, BundlesApiClient},
	steps::{AppendManyOrders, AppendOneOrder},
};

#[derive(Debug, Clone)]
pub enum Order<P: Platform> {
	/// A single transaction.
	Transaction(Recovered<types::Transaction<P>>),
	/// A bundle of transactions.
	Bundle(types::Bundle<P>),
}

impl<P: Platform> Order<P> {
	pub fn hash(&self) -> B256 {
		match self {
			Order::Transaction(tx) => *tx.tx_hash(),
			Order::Bundle(bundle) => bundle.hash(),
		}
	}

	pub fn transactions(&self) -> &[Recovered<types::Transaction<P>>] {
		match self {
			Order::Bundle(bundle) => bundle.transactions(),
			Order::Transaction(tx) => core::slice::from_ref(tx),
		}
	}
}
