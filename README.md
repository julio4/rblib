# rblib — A Modular Rust SDK for Ethereum Block Building

[![Sanity Check](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml/badge.svg)](https://github.com/flashbots/rblib/actions/workflows/sanity.yaml)


> [\!WARNING]
> This library is **alpha software** under active development. Breaking changes are expected, APIs are not yet stable, and no official support is provided. Early adopters are encouraged to open issues for bugs and feedback to help shape the project's direction.

![](./assets/rblib_chip.png)


**rblib** is a high-performance, modular Rust SDK for constructing Ethereum-compatible block builders. Built on top of the [Reth](https://github.com/paradigmxyz/reth) execution engine, it provides robust, platform-agnostic primitives and declarative workflows designed for both L1 and L2 applications.

-----

## Core Philosophy

`rblib` is designed for sophisticated engineering teams who require granular control and composability in their block-building logic. Our philosophy centers on three key principles:

  * **Modularity:** Core components are decoupled. Use the low-level Payload API for fine-grained control, or compose high-level Pipelines for declarative workflows.
  * **Performance:** By leveraging Reth and a zero-copy design for payload state, `rblib` is built for high-throughput, latency-sensitive environments.
  * **Extensibility:** A clean `Platform` trait abstraction allows for seamless extension to support custom EVM-based chains (L2s, app-chains) without forking the core logic.

-----

## Key Features

  * **Composable Payload API:** A flexible, low-level API for constructing and inspecting block payloads via immutable `Checkpoint` transformations.
  * **Declarative Pipelines API:** A high-level, composable system for defining block-building workflows (e.g., ordering, revert protection) as reusable `Step`s.
  * **Platform-Agnostic Design:** Out-of-the-box support for `Ethereum` and `Optimism`, with a clear interface for adding new platforms.
  * **Integrated Testing Framework:** A rich test suite with utilities for multi-platform testing, local Reth nodes, and isolated component tests.

-----

## Getting Started

Add `rblib` to your project's dependencies:

```bash
cargo add rblib
```

-----

## Core Concepts

`rblib` is split into two primary APIs that can be used independently or together.

### 1\. The Payload API

Located in `rblib::payload` (see `src/payload`), this API provides the foundational primitives for block construction. It enables exploring many different payload variations efficiently.

#### Checkpoints

A `Checkpoint<P>` is an immutable snapshot of a payload's state after a mutation (e.g., applying a transaction or bundle). Checkpoints are cheap to clone and fork, making it trivial to explore alternative block constructions from a common state. Each checkpoint retains the history of its parent, allowing it to act as a `DatabaseRef` for the chain state at that specific point. Common algorithms for inspecting checkpoints are available in `src/payload/ext/checkpoint.rs`.

**Example: Building and Forking a Payload**

```rust
use rblib::{*, test_utils::*};

let ctx = BlockContext::<Ethereum>::mocked();

// Create a linear history
let checkpoint1 = ctx.apply(tx1)?;
let checkpoint2 = checkpoint1.apply(tx2)?;

// Fork from an earlier state to explore an alternative
let checkpoint3_alt = checkpoint1.apply(tx3)?;

// Compare the outcomes of the two different forks
let gas_main = checkpoint2.cumulative_gas_used();
let gas_alt = checkpoint3_alt.cumulative_gas_used();
let balance_main = checkpoint2.balance_of(coinbase_addr);
let balance_alt = checkpoint3_alt.balance_of(coinbase_addr);
```

#### Spans

A `Span<P>` is a view over a linear sequence of checkpoints, useful for analyzing or manipulating a specific portion of a payload's history.

```rust
use rblib::payload::*;

let full_history = checkpoint.history();
let total_gas = full_history.gas_used();

// Analyze a sub-section of the payload
let sub_span = full_history.skip(2).take(4);
let sub_gas = sub_span.gas_used();
```

### 2\. The Pipelines API

Located in `rblib::pipelines` (see `src/pipelines/`), this API uses the Payload API to create declarative, reusable block-building workflows. It is particularly powerful for L2s, where standardized logic can be composed and customized.

Pipelines are built from **Steps** (`src/pipelines/step.rs`) and control-flow components (`src/pipelines/mod.rs`). A rich library of common, reusable steps is provided in `src/pipelines/steps/`.

**Example: A Minimal Builder Pipeline**

This example defines a pipeline that loops over a set of ordering and protection steps before finalizing the block.

```rust
use rblib::*;

fn main() {
    // Define a reusable pipeline workflow
    let pipeline = Pipeline::<Ethereum>::default()
        .with_epilogue(BuilderEpilogue)
        .with_pipeline(
            Loop,
            (
                AppendOneOrder::default(),
                PriorityFeeOrdering,
                TotalProfitOrdering,
                RevertProtection,
            ),
        );

    // Integrate with a Reth node
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

-----

## Platform Abstraction

Chain-specific logic (e.g., transaction types, block validation rules) is abstracted via the `Platform` trait (`src/platform/mod.rs`). This allows the core building logic to remain generic while providing concrete implementations for `Ethereum` and `Optimism`.

To support a custom chain, implement the `Platform` trait. See `examples/custom-platform.rs` for a practical example.

-----

## Examples and Integration

`Pipeline::into_service()` is the primary entry point for converting a pipeline into a Reth-compatible `PayloadServiceBuilder`. For complete integration examples, see the `examples/` directory and the reference builder implementation in `bin/flashblocks/src/main.rs`.

-----

## Development & Testing

The project includes a comprehensive testing infrastructure to ensure reliability.

  * **Run all tests:**
    ```bash
    cargo test
    ```
  * **Run a specific test with verbose logging:**
    ```bash
    TEST_TRACE=on cargo test smoke::all_transactions_included_ethereum
    ```
- **Multi-Platform Testing**: Use `#[rblib_test(Ethereum, Optimism, YourCustomPlatform)]` to run tests across platforms.
- **Local Test Nodes**: `LocalNode<P, C>` (`src/test_utils/node.rs`) provides full Reth nodes for integration testing.
- **Step Testing**: `OneStep<P>` (`src/test_utils/step.rs`) enables isolated step testing.
- **Funded Accounts**: Use `FundedAccounts::by_address()` or `.with_random_funded_signer()` for test transactions.
- **Mocks**: Mocking utilities are available for types like `BlockContext` and `PayloadAttributes`.


-----

## Contributing

Contributions are welcome. Please feel free to open an issue to discuss a bug, feature request, or design question. Pull requests should be focused and include relevant tests.

## License

This project is licensed under the [MIT License](/LICENSE).

-----

Made with ☀️ by the ⚡🤖 collective.