use {
	derive_more::{Deref, From, FromStr, Into},
	rblib::alloy::signers::local::PrivateKeySigner,
};

/// This type is used to store the builder's secret key for signing the last
/// transaction in the block.
#[derive(Debug, Clone, Deref, FromStr, Into, From)]
pub struct BuilderSigner {
	signer: PrivateKeySigner,
}

impl PartialEq for BuilderSigner {
	fn eq(&self, other: &Self) -> bool {
		self.signer.address() == other.signer.address()
	}
}
impl Eq for BuilderSigner {}
