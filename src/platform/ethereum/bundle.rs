use {
	crate::{
		Ethereum,
		alloy::primitives::TxHash,
		reth::primitives::Recovered,
		uuid::Uuid,
		*,
	},
	core::convert::Infallible,
};

#[derive(Debug, Clone)]
pub struct EthereumBundle;
impl Bundle<Ethereum> for EthereumBundle {
	type PostExecutionError = Infallible;

	fn transactions(&self) -> &[Recovered<types::Transaction<Ethereum>>] {
		&[]
	}

	fn without_transaction(self, _tx: TxHash) -> Self {
		todo!()
	}

	fn is_eligible(&self, _block: &BlockContext<Ethereum>) -> Eligibility {
		todo!()
	}

	fn is_allowed_to_fail(&self, _tx: TxHash) -> bool {
		todo!()
	}

	fn is_optional(&self, _tx: TxHash) -> bool {
		todo!()
	}

	fn uuid(&self) -> Uuid {
		todo!()
	}
}
