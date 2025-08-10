use {
	crate::{alloy, prelude::*, reth},
	alloy::consensus::BlockHeader,
	metrics::{Counter, Histogram},
	metrics_derive::Metrics,
	reth::node::builder::BuiltPayload,
};

#[derive(Metrics)]
#[metrics(scope = "rblib_payload")]
pub struct Payload {
	/// Number of new payload jobs that have started.
	pub jobs_started: Counter,

	/// Number of payload jobs that have produced a block payload successfully.
	pub jobs_completed: Counter,

	/// Number of payload jobs that have started and failed to produce a block
	/// payload.
	pub jobs_failed: Counter,

	/// Duration of a payload job from start to completion or failure.
	pub job_duration: Histogram,

	/// Number of transactions in a produced payload.
	pub tx_count_histogram: Histogram,

	/// Total number of transactions in all produced payloads.
	pub tx_count_total: Counter,

	/// Amount of gas used by transactions in a produced payload.
	pub gas_used_histogram: Histogram,

	/// Total gas used by all produced payloads.
	pub gas_used_total: Counter,

	/// The percentage of gas utilized by transactions in a produced payload.
	pub gas_utilized: Histogram,

	/// Amount of gas used by blob transactions in a produced payload.
	pub blob_gas_histogram: Histogram,

	/// Total blob gas used by all blob transactions in all produced payloads.
	pub blob_gas_total: Counter,

	/// The percentage of gas utilized by blob transactions in a produced
	/// payload.
	pub blob_gas_utilized: Histogram,

	/// Fees paid by transactions in a produced payload.
	pub fees_histogram: Histogram,

	/// Total fees accumulated by all produced payloads.
	pub fees_total: Counter,
}

impl Payload {
	#[allow(clippy::cast_precision_loss)]
	pub fn record_payload<P: Platform>(
		&self,
		payload: &types::BuiltPayload<P>,
		block: &BlockContext<P>,
	) {
		let fees: f64 = payload.fees().into();

		self.fees_histogram.record(fees);
		self.fees_total.increment(payload.fees().to::<u64>());

		let gas_used = payload.block().gas_used() as f64;
		let gas_limit = payload.block().gas_limit() as f64;
		let gas_utilization = gas_used / gas_limit;

		self.gas_used_histogram.record(gas_used);
		self.gas_used_total.increment(payload.block().gas_used());
		self.gas_utilized.record(gas_utilization);

		let blob_gas_used = payload.block().blob_gas_used().unwrap_or_default();
		let blob_gas_limit = block.blob_gas_limit();
		let blob_gas_utilization = blob_gas_used as f64 / blob_gas_limit as f64;

		self.blob_gas_histogram.record(blob_gas_used as f64);
		self.blob_gas_total.increment(blob_gas_used);
		self.blob_gas_utilized.record(blob_gas_utilization);

		self
			.tx_count_histogram
			.record(payload.block().transaction_count() as f64);
		self
			.tx_count_total
			.increment(payload.block().transaction_count() as u64);
	}
}
