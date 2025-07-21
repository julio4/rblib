# rblib - Builder SDK for Ethereum Block Builders

rblib is a Rust SDK for building Ethereum-compatible block builders using the Reth execution engine.

## Core Architecture

**Platform-Agnostic Design**: The codebase uses a `Platform` trait (`src/platform/mod.rs`) to abstract Ethereum vs Optimism implementations. All core types are parameterized by `P: Platform`.

**Payload Building API**: Core abstraction in `src/payload/`:
- `BlockContext<P>`: Immutable block template with parent header, attributes, chainspec
- `Checkpoint<P>`: Mutable payload state that can be forked/modified
- `Span<P>`: Represents a sequence of checkpoints with linear history.

**Pipeline-Based Building**: Block construction follows a declarative pipeline pattern:
- **Steps** (`src/pipelines/step.rs`): Individual units implementing `Step<P>` trait with `step()`, `before_job()`, `after_job()` methods
- **Pipelines** (`src/pipelines/mod.rs`): Composed using builder pattern with `.with_step()`, `.with_prologue()`, `.with_epilogue()`
- **Execution** (`src/pipelines/exec/`): State machine that orchestrates step execution with proper error handling

The payload building API can be used independently of the pipeline system. The Pipelines api relies on the payload API and provides a higher-level abstraction for composing payload building steps.

## Testing Infrastructure

**Multi-Platform Testing**: Use `#[rblib_test(Ethereum, Optimism)]` macro to run tests across platforms automatically.

**Local Test Nodes**: `LocalNode<P, C>` in `src/test_utils/node.rs` provides full reth nodes for integration testing:
- Automatically manages temporary data directories with cleanup
- IPC-based RPC connection to deployed pipeline
- `ConsensusDriver` trait simulates CL-EL protocol for payload building

**Step Testing**: `OneStep<P>` utility (`src/test_utils/step.rs`) enables isolated step testing with realistic node environment.

**Funded Accounts**: Use `FundedAccounts::by_address()` or `.with_random_funded_signer()` for test transactions.

## Development Commands

```bash
# Run all tests
cargo test

# Debug specific test with full logging
TEST_TRACE=on cargo test smoke::all_transactions_included_ethereum
```

## Integration with Reth

The SDK integrates with Reth through:
- `Pipeline::into_service()` converts pipelines to `PayloadServiceBuilder`
- Platform-specific `construct_payload()` methods use Reth's native builders
- Type boundaries defined by `traits::PoolBounds`, `traits::ProviderBounds`, etc.

See `examples/` and `bin/op2/src/main.rs` for complete integration examples.

## External Dependencies Context

**Critical External Repositories**: For comprehensive understanding, consult these upstream dependencies:

- **Reth** (https://github.com/paradigmxyz/reth): Ethereum execution client that rblib builds upon. Essential for understanding node architecture, payload building interfaces, and EVM execution patterns.

- **Alloy** (https://github.com/alloy-rs/alloy): Ethereum library providing types, RPC interfaces, and network abstractions. Critical for transaction handling, network communication, and type definitions used throughout rblib.

Check platform-specific considerations in `src/platform/{ethereum,optimism}/` for L1 vs L2 differences.
