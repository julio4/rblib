# Builder SDK

SDK for building payload builders.


## Example

Minimal library usage example that creates a fully functional block builder using a reth node for ethereum mainnet:

```rust
use rblib::*;

fn main() {
  let pipeline = Pipeline::default()
    .with_epilogue(BuilderEpilogue)
    .with_step(GatherBestTransactions)
    .with_step(PriorityFeeOrdering)
    .with_step(TotalProfitOrdering)
    .with_step(RevertProtection);

  Cli::parse_args()
    .run(|builder, _| {
      let handle = builder
        .with_types::<EthereumNode>()
        .with_components(EthereumNode::components().payload(pipeline.into_service()))
        .with_add_ons(EthereumAddOns::default())
        .launch()
        .await?;

      handle.wait_for_node_exit().await
    })
    .unwrap();
}
```
