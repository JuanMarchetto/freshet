# freshet

**Resumable, permissionless, sharded MapReduce over Solana accounts** — the
framework-agnostic core.

A Solana transaction can only lock ~128 accounts and spend 1.4M CU, so one event cannot
atomically modify an unbounded set of accounts. `freshet` is the on-chain pattern that
lifts that ceiling: a single logical effect is partitioned into **permissionless,
resumable, idempotent crank batches**, optionally in two phases:

- **REDUCE** — scan N members into a commutative-monoid accumulator (tally, winner, …);
- **MAP** — apply the result to every member, *including members whose owner never shows
  up* (push-mode) — which a pull-based "claim it yourself" design cannot express.

Exactly-once is guaranteed by a monotonic cursor within an epoch plus a per-member epoch
stamp across re-runs; liveness by a keeper bounty. See the
[SPEC](https://github.com/JuanMarchetto/freshet/blob/main/SPEC.md) for the full contract.

## What this crate is

The pure, `no_std`, alloc-free logic with **no Solana dependency**:

- `partition` — the deterministic member→shard partition (computed, never stored).
- `state` — the verified state-machine guards (`advance_step`, …) that drive every phase
  transition, plus a host-only executable model (`Machine`, behind the `std` feature) that
  the property tests model-check.
- `monoid` — the `Monoid` trait for the REDUCE accumulator, with `Sum` and `MaxWinner`.

`freshet` is a **library linked into the consumer program** (only the owning program may
write an account). The consumer owns its account layout and delegates control-flow to these
guards, so its handlers run the verified logic. Reference settlers in **Pinocchio, Anchor,
and Quasar** — with a cross-framework CU benchmark — live in the
[repository](https://github.com/JuanMarchetto/freshet).

## Usage

```toml
[dependencies]
freshet = { version = "0.1", default-features = false } # no_std, for on-chain programs
```

```rust
use freshet::partition::{partition_len, partition_start};
use freshet::monoid::{Monoid, Sum};

assert_eq!(partition_len(10, 3, 0), 4); // 10 members over 3 shards → 4 + 3 + 3
assert_eq!(partition_start(10, 3, 1), 4);
assert_eq!(Sum(3).combine(Sum(4)).combine(Sum(0)), Sum(7));
```

The default `std` feature enables the host-only `Machine` model used by the tests; on-chain
programs link with `default-features = false`.

## Status

Unaudited; the reference on-chain programs use a placeholder program `ID` — do not deploy
as-is. Licensed under MIT.
