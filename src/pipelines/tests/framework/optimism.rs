use crate::{Optimism, pipelines::tests::NetworkSelector};

impl NetworkSelector for Optimism {
	type Network = op_alloy::network::Optimism;
}

// pub struct OptimismConsensusDriver;
// impl ConsensusDriver<Optimism> for OptimismConsensusDriver {
// 	type Params = ();

// 	async fn start_building(
// 		&self,
// 		node: &LocalNode<Optimism, Self>,
// 		target_timestamp: u64,
// 		(): &Self::Params,
// 	) -> eyre::Result<PayloadId> {
// 		todo!()
// 	}

// 	async fn finish_building(
// 		&self,
// 		node: &LocalNode<Optimism, Self>,
// 		payload_id: PayloadId,
// 		(): &Self::Params,
// 	) -> eyre::Result<Block> {
// 		todo!()
// 	}
// }
