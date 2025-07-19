use crate::{
	Ethereum,
	alloy::primitives::TxHash,
	reth::primitives::Recovered,
	uuid::Uuid,
	*,
};

#[derive(Debug, Clone)]
pub struct EthereumBundle;
impl Bundle<Ethereum> for EthereumBundle {
	fn transactions(&self) -> &[Recovered<types::Transaction<Ethereum>>] {
		&[]
	}

	fn without_transaction(_tx: TxHash) -> Self {
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
