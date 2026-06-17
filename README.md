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
program/   freshet-program — Pinocchio (no_std) reference settler. layout (zero-copy Pod,
           compile-time offset asserts), error map, §11 security helpers, 16 instructions.
royale/    freshet-royale — a battle-royale demo consumer of the core: each round
           eliminates every player who didn't act, INCLUDING offline ones (push-mode,
           which a pull-based "claim-it-yourself" design cannot express).
anchor/    freshet-anchor — Anchor port of the benchmark path (same byte layout +
           core delegation), for the cross-framework CU comparison.
quasar/    freshet-quasar — Quasar port of the benchmark path. Standalone workspace
           (own Cargo.lock); turns out the cheapest of the three (see BENCHMARK.md).
SPEC.md    the authoritative contract.  DESIGN.md / GAMES.md  strategy + demo eval.
BENCHMARK.md  measured Pinocchio + Anchor + Quasar compute units, cross-framework.
```

## Build & test

```bash
cargo test                                            # core + program + royale (host + mollusk)
cargo build-sbf --manifest-path program/Cargo.toml    # the deployable settler .so
cargo build-sbf --manifest-path royale/Cargo.toml     # the demo .so
SBF_OUT_DIR=target/deploy cargo test -p freshet-program --test bench_cu -- --nocapture  # CU report
```

The reference settler uses a concrete demo consumer (each member is a `u64`, the effect
credits every account by a delta). `royale` is a second consumer (player elimination).
Both reuse the verified `freshet` core guards; a real consumer implements its own
`apply`/`reduce`.

## Status

Core + the full 16-instruction program are implemented and build for SBF; mollusk
integration tests cover the full single-pass lifecycle to `Done` and the cross-escrow-drain
rejection. The benchmark path is ported to all three frameworks (Pinocchio, Anchor, Quasar)
and measured (`BENCHMARK.md`), and `freshet-royale` is a working demo consumer. Deferred:
System-CPI account creation (handlers assume pre-allocated PDAs), pull mode (gated off), and
the broader test matrix + compute-unit regression gate. Not audited; **`ID` is a
placeholder** — do not deploy as-is.

## License

MIT — see [LICENSE](LICENSE).
