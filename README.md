# rblib — Rust SDK for Ethereum Block Builders

[![Sanity Check](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml/badge.svg)](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml)

**rblib** is a Rust SDK for building Ethereum-compatible block builders using the [Reth](https://github.com/paradigmxyz/reth) execution engine. It provides two independent, platform-agnostic APIs:

- **Payload API** — Flexible primitives for constructing and inspecting block payloads.
- **Pipelines API** — Declarative workflows for block building, suitable for both L1 and L2 scenarios.

Out-of-the-box support is provided for both `Ethereum` and `Optimism` platforms via the `Platform` trait.

---

## Payload Building API

Located in `src/payload`, this API offers a composable interface for building block payloads through a series of checkpoints. It can be used standalone, without the pipeline system.

### Checkpoints

A `Checkpoint<P>` is an atomic unit of payload mutation—think of it as a state transformation (e.g., applying a transaction or bundle). Checkpoints are cheap to copy, discard, and fork, making it easy to explore alternative payload constructions.

Each checkpoint:

- Tracks its entire history back to the block’s initial state.
- Implements `DatabaseRef`, acting as a chain state provider rooted at the parent block plus all mutations in its history.

Common algorithms for building and inspecting checkpoints are available in `src/payload/ext/checkpoint.rs`.

#### Example: Building and Forking Payloads

```rust
use rblib::{*, test_utils::*};

let ctx = BlockContext::<Ethereum>::mocked();

let checkpoint1 = ctx.apply(tx1)?;
let checkpoint2 = checkpoint1.apply(tx2)?;

// Fork the state to explore alternatives
let checkpoint3_alt = checkpoint1.apply(tx3)?;

// Compare alternative payloads
let gas1 = checkpoint2.cumulative_gas_used();
let gas2 = checkpoint3_alt.cumulative_gas_used();

let balance1 = checkpoint2.balance_of(coinbase_addr);
let balance2 = checkpoint3_alt.balance_of(coinbase_addr);
```

### Spans

A `Span<P>` represents a linear sequence of checkpoints—useful for analyzing or manipulating parts of a payload.

```rust
use rblib::payload::*;

let all_history = checkpoint.history();
let all_gas = all_history.gas_used();

let sub_span = all_history.skip(2).take(4);
let sub_gas = sub_span.gas_used();
```

### Platform Agnostic

The Payload API works with any type implementing the `Platform` trait.

---

## Pipelines API

Located in `src/pipelines/`, this API builds on the Payload API to provide a declarative, composable workflow for block building. It’s especially useful for L2 builders, where common logic can be reused and customized.

### Example: Minimal Builder Pipeline

```rust
use rblib::*;

fn main() {
    let pipeline = Pipeline::<Ethereum>::default()
        .with_epilogue(BuilderEpilogue)
        .with_pipeline(
            Loop,
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
                    EthereumNode::components()
                        .payload(pipeline.into_service()))
                .with_add_ons(EthereumAddOns::default())
                .launch()
                .await?;

            handle.wait_for_node_exit().await
        })
        .unwrap();
}
```

### Pipeline Pattern

- **Steps** (`src/pipelines/step.rs`): Each `Step<P>` implements methods like `step()`, `before_job()`, and `after_job()`. Common steps include transaction ordering, revert protection, and more.
- **Pipelines** (`src/pipelines/mod.rs`): Compose steps and control-flow components using a builder pattern (`.with_step()`, `.with_prologue()`, `.with_epilogue()`).
- **Common Steps Library** (`src/pipelines/steps/`): A collection of well-tested reusable payload building steps commonly used in most block builders.

---

## Platform Abstraction Layer

Chain-specific logic is separated via the `Platform` trait (`src/platform/mod.rs`). You can customize platforms for different EVMs, block structures, or transaction types. See `examples/custom-platform.rs` for extending Optimism with custom bundles.

Default implementations for `Ethereum` and `Optimism` are included.

---

## Testing Infrastructure

- **Multi-Platform Testing**: Use `#[rblib_test(Ethereum, Optimism, YourCustomPlatform)]` to run tests across platforms.
- **Local Test Nodes**: `LocalNode<P, C>` (`src/test_utils/node.rs`) provides full Reth nodes for integration testing.
- **Step Testing**: `OneStep<P>` (`src/test_utils/step.rs`) enables isolated step testing.
- **Funded Accounts**: Use `FundedAccounts::by_address()` or `.with_random_funded_signer()` for test transactions.
- **Mocks**: Mocking utilities are available for types like `BlockContext` and `PayloadAttributes`.

---

## Development Commands

Run all tests:

```bash
cargo test
```

Debug a specific test with logs:

```bash
TEST_TRACE=on cargo test smoke::all_transactions_included_ethereum
```

---

## Integration with Reth

- `Pipeline::into_service()` converts pipelines to `PayloadServiceBuilder`.
- Platform-specific `build_payload()` methods use Reth’s native builders.

See `examples/` and `bin/flashblocks/src/main.rs` for integration examples.

---

## Contributing

Contributions, issues, and feature requests are welcome. Please see the codebase and examples for guidance on extending platforms, steps, or pipelines.
