# freshet

A resumable, permissionless, shardable **MapReduce over Solana accounts**. One on-chain
event can modify an unbounded number of accounts — settling a prediction market to all
winners, ticking a world of game entities, mass payouts — by partitioning the work into
permissionless, resumable, idempotent **crank batches**, with a complementary pull (lazy)
mode for formula effects.

It exists because a Solana transaction must declare every account it touches and can lock
only ~128 of them, so large state fan-out has to be spread across many transactions. The
name: a *freshet* is a sudden river surge fed by many tributaries.

## Why it's interesting

- **The state machine is a model-checked, executable spec.** `core/src/state.rs` is a pure
  (no-`std`, no Solana) reference model — init/enroll/seal, the REDUCE→MAP engine, lazy
  per-epoch reset, cursor-derived completion. Its property tests (`cargo test`) include an
  exhaustive liveness sweep ("every sealed effect reaches Done, exactly-once coverage") and
  the skip-non-wedge cases. The on-chain handlers **delegate** their control-flow to the
  same verified guard functions, so the program runs the logic the tests verify.
- **Zero-trust by design.** The rule lives in an account; anyone can crank; every transition
  is verifiable on-chain. No trusted operator. The keeper-bounty escrow is bound to its
  `(effect, shard_id)` (a cross-escrow drain is rejected — see the integration test).
- **Hardened by adversarial review.** The spec went through several multi-agent red-team
  rounds + a state-machine model-check; the fixes (cursor-derived completion, the universal
  lazy-reset prologue, phase-aware funding, the escrow binding, …) are documented in
  `SPEC.md`'s changelog.

## Layout

```
core/      freshet — framework-agnostic crate: partition (§2.8), state machine + engine,
           monoid. no_std; `Machine` model behind the `std` feature. 31 tests.
program/   freshet-program — Pinocchio (no_std) on-chain program. layout (zero-copy Pod,
           compile-time offset asserts), error map, §11 security helpers, 16 instructions.
SPEC.md    the authoritative contract (v0.6).  DESIGN.md / GAMES.md  strategy + demo eval.
```

## Build & test

```bash
cargo test                  # core (31) + program unit + mollusk integration (host)
cargo build-sbf --manifest-path program/Cargo.toml   # the deployable .so
```

The reference settler uses a concrete demo consumer (each member is a `u64`, the effect
credits every account by a delta). A real consumer implements its own `apply`/`reduce`.

## Status

Core + the full 16-instruction program are implemented and build for SBF; mollusk
integration tests cover the full single-pass lifecycle to `Done` and the cross-escrow-drain
rejection. Deferred: System-CPI account creation (handlers assume pre-allocated PDAs), pull
mode (gated off), the broader test matrix + compute-unit regression gate, and the
`freshet-royale` demo game. Not audited; **`ID` is a placeholder** — do not deploy as-is.

## License

MIT — see [LICENSE](LICENSE).
