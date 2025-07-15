# Builder SDK

[![Sanity Check](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml/badge.svg)](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml)

SDK for building payload builders.

## Example

Minimal library usage example that creates a fully functional block builder using a reth node for Ethereum mainnet:

```rust
use rblib::*;

fn main() {
 let pipeline = Pipeline::<Ethereum>::default()
  .with_epilogue(BuilderEpilogue)
  .with_pipeline(Loop,
    (
      AppendOneTransactionFromPool::default(),
      PriorityFeeOrdering,
      TotalProfitOrdering,
      RevertProtection,
    ),
  );

 Cli::parse_args()
  .run(|builder, _| async move {
   let handle = builder
    .with_types::<EthereumNode>()
    .with_components(
     EthereumNode::components().payload(pipeline.into_service()),
    )
    .with_add_ons(EthereumAddOns::default())
    .launch()
    .await?;

   handle.wait_for_node_exit().await
  })
  .unwrap();
}
```


## Testing and debugging

Running all tests in the workspace:

```terminal
cargo test
```

Debugging a specific test with output logs:

```terminal
TEST_TRACE=on cargo test smoke::all_transactions_included_ethereum
```
