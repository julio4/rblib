use {
	alloy::primitives::{Address, B256, Signature, U256},
	secp256k1::{Message, PublicKey, SECP256K1, SecretKey},
	sha3::{Digest, Keccak256},
	std::str::FromStr,
};

/// Simple struct to sign txs/messages.
/// Mainly used to sign payout txs from the builder and to create test data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signer {
	pub address: Address,
	pub pubkey: PublicKey,
	pub secret: SecretKey,
}

impl Signer {
	pub fn try_from_secret(secret: B256) -> Result<Self, secp256k1::Error> {
		let secret = SecretKey::from_byte_array(secret.into())?;
		let pubkey = secret.public_key(SECP256K1);
		let address = public_key_to_address(&pubkey);

		Ok(Self {
			address,
			pubkey,
			secret,
		})
	}

	pub fn sign_message(
		&self,
		message: B256,
	) -> Result<Signature, secp256k1::Error> {
		let s = SECP256K1.sign_ecdsa_recoverable(
			Message::from_digest(message.into()),
			&self.secret,
		);
		let (rec_id, data) = s.serialize_compact();

		let signature = Signature::new(
			U256::try_from_be_slice(&data[..32])
				.expect("The slice has at most 32 bytes"),
			U256::try_from_be_slice(&data[32..64])
				.expect("The slice has at most 32 bytes"),
			i32::from(rec_id) != 0,
		);
		Ok(signature)
	}

	#[allow(dead_code)]
	pub fn random() -> Self {
		Self::try_from_secret(B256::random())
			.expect("failed to create random signer")
	}
}

impl FromStr for Signer {
	type Err = eyre::Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Self::try_from_secret(B256::from_str(s)?)
			.map_err(|e| eyre::eyre!("invalid secret key {:?}", e.to_string()))
	}
}

/// Converts a public key to an Ethereum address
pub fn public_key_to_address(public_key: &PublicKey) -> Address {
	// Get uncompressed public key (65 bytes: 0x04 + 64 bytes)
	let pubkey_bytes = public_key.serialize_uncompressed();

	// Skip the 0x04 prefix and hash the remaining 64 bytes
	let hash = Keccak256::digest(&pubkey_bytes[1..65]);

	// Take last 20 bytes as address
	Address::from_slice(&hash[12..32])
}
