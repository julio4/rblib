use {
	crate::{alloy::primitives::TxHash, reth::primitives::Recovered, *},
	core::convert::Infallible,
};

#[derive(Debug, Clone)]
pub struct OpBundle;
impl Bundle<Optimism> for OpBundle {
	type PostExecutionError = Infallible;

	fn transactions(&self) -> &[Recovered<types::Transaction<Optimism>>] {
		&[]
	}

	fn without_transaction(self, _: TxHash) -> Self {
		todo!()
	}

	fn is_eligible(&self, _block: &BlockContext<Optimism>) -> Eligibility {
		todo!()
	}

	fn is_allowed_to_fail(&self, _tx: TxHash) -> bool {
		todo!()
	}

	fn is_optional(&self, _tx: TxHash) -> bool {
		todo!()
	}
}
