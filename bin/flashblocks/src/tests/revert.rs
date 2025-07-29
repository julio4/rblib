#[tokio::test]
async fn when_disabled_reverted_txs_are_included() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn bundle_with_one_reverted_tx_not_included() -> eyre::Result<()> {
	todo!()
}

#[tokio::test]
async fn bundle_with_one_reverting_tx_allowed_to_revert_included()
-> eyre::Result<()> {
	todo!()
}

/// If a transaction reverts and gets dropped it, the eth_getTransactionReceipt
/// should return an error message that it was dropped.
#[tokio::test]
async fn reverted_dropped_tx_has_valid_receipt_status() -> eyre::Result<()> {
	todo!()
}
