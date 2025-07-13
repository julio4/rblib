use alloy::{
	network::EthereumWallet,
	primitives::Address,
	signers::local::PrivateKeySigner,
};

/// Those accounts are defined in the gensis block of the test local node,
/// each prefunded with 100 ETH and nonces starting from 0.
pub struct FundedAccounts;

#[allow(clippy::cast_possible_truncation)]
impl FundedAccounts {
	const FUNDED_PRIVATE_KEYS: &[&str] = &[
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff81",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff82",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff83",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff84",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff85",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff86",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff87",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff88",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff89",
		"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff90",
	];

	pub fn signer(key: u32) -> PrivateKeySigner {
		assert!(
			(key as usize) < Self::FUNDED_PRIVATE_KEYS.len(),
			"Key index out of bounds, must be less than {}",
			Self::FUNDED_PRIVATE_KEYS.len()
		);

		Self::FUNDED_PRIVATE_KEYS[key as usize]
			.parse()
			.expect("invalid hardcoded builder private key")
	}

	pub fn address(key: u32) -> Address {
		Self::signer(key).address()
	}

	pub fn random() -> PrivateKeySigner {
		let key = rand::random::<u32>() % Self::FUNDED_PRIVATE_KEYS.len() as u32;
		Self::signer(key)
	}

	pub fn addresses() -> impl Iterator<Item = Address> {
		Self::FUNDED_PRIVATE_KEYS
			.iter()
			.enumerate()
			.map(|(i, _)| Self::address(i as u32))
	}

	pub fn signers() -> impl Iterator<Item = PrivateKeySigner> {
		Self::FUNDED_PRIVATE_KEYS
			.iter()
			.enumerate()
			.map(|(i, _)| Self::signer(i as u32))
	}

	pub fn by_address(address: Address) -> Option<PrivateKeySigner> {
		Self::signers().find(|signer| signer.address() == address)
	}

	pub fn wallet(key: u32) -> EthereumWallet {
		EthereumWallet::new(Self::signer(key))
	}

	pub fn wallet_by_address(address: Address) -> Option<EthereumWallet> {
		Self::by_address(address).map(EthereumWallet::new)
	}
}
