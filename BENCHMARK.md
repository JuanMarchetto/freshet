# freshet — compute-unit benchmark

A primitive that cranks state across many accounts lives or dies on its per-member
compute cost, so freshet treats CU as a first-class, measured property — not a claim.

> **Honesty up front.** Only the **Pinocchio** implementation exists today, so only its
> column has real numbers. The Quasar and Anchor columns are **not yet implemented** and
> are left empty on purpose — no estimated or hand-waved figures. The cross-framework
> comparison is the *plan* (§ "Cross-framework", below); the methodology and harness here
> are built so those columns fill in with apples-to-apples numbers once the ports land.

## Methodology

- Harness: [`mollusk-svm`](https://github.com/anza-xyz/mollusk) 0.13, single-instruction
  execution against the built `.so`, reading `InstructionResult::compute_units_consumed`.
- The reference consumer's `apply` is a trivial saturating add, so the reported figures
  are **freshet's own overhead** — per-member cost is dominated by membership verification
  (the canonical-PDA check), not domain logic. A real consumer's `apply`/`reduce` cost
  stacks on top of these.
- Reproduce:

  ```bash
  cargo build-sbf --manifest-path program/Cargo.toml
  SBF_OUT_DIR=target/deploy cargo test -p freshet-program --test bench_cu -- --nocapture
  ```

- Measured on Solana platform-tools v1.52 (agave 3.1), single shard.

## Results — Pinocchio (measured)

| instruction | CU | notes |
|---|--:|---|
| `init_effect` | 155 | Effect header write only |
| `init_shards` (1 shard) | 3 427 | two PDA derivations (shard + escrow) + writes |
| `enroll` (1 member) | 1 754 | one member-PDA derivation + header write |
| `seal` | 122 | Effect-only transition + guards |
| `advance_apply` (batch = 2) | 7 212 | escrow binding + 2 members + payout |
| `try_finish_apply` (1 shard) | 173 | cursor-derived completion check |
| `finalize` | 78 | flag set |

### `advance_apply` scaling (the hot path)

| batch | CU |
|--:|--:|
| 1 | 5 526 |
| 2 | 7 212 |
| 4 | 10 578 |
| 8 | 17 309 |

Least-squares fit: **overhead ≈ 3 844 CU + ≈ 1 683 CU per member.**

- The **fixed overhead** (~3.8k) is the Effect/Shard loads + the `(effect, shard_id)`
  escrow binding (itself a `create_program_address`) + the rent-floor read.
- The **per-member** cost (~1.7k) is almost entirely the canonical-PDA check
  (`create_program_address`) that binds each supplied account to its index. That check is
  the load-bearing, zero-trust membership guarantee — dropping it was the one review
  finding rejected as a real vulnerability — so this is the *price of permissionless
  safety*, paid once per member, amortized by larger batches against the fixed overhead.

### Practical batch sizing

For a trivial `apply`, the per-tx batch is bounded by Solana's ~128 writable-account lock
limit (≈120 members/tx with ALT), not by CU (≈120 members ≈ 3.8k + 120·1.7k ≈ 208k CU,
well under the 1.4M/tx budget). A heavy `apply` (e.g. a token transfer per member) becomes
CU-bound first and forces a smaller batch — `apply` cost, not freshet overhead, is then the
limiter. This is why §14 of `SPEC.md` reports overhead separately from total and forbids
any unqualified "deterministic CU per crank" claim.

## Cross-framework (planned)

The headline artifact is the *same primitive* in three frameworks, benchmarked identically.

| instruction | Pinocchio | Quasar | Anchor |
|---|--:|--:|--:|
| `advance_apply` overhead | 3 844 CU | — | — |
| `advance_apply` per-member | 1 683 CU | — | — |
| `init_shards` | 3 427 CU | — | — |

Status and expectation (to be confirmed by measurement, not asserted):

- **Quasar** — zero-copy/`no_std` like Pinocchio; expected to land close to these numbers.
  Its typed `remaining_accounts` caps at 64, so `reduce_shards`/`enroll_batch` would use the
  raw accessor (see `SPEC.md` §15.1). *Not yet implemented.*
- **Anchor** — IDL/ergonomics at a CU premium; the interesting question is *how much* the
  account-deserialization overhead adds versus the hand-rolled zero-copy path. *Not yet
  implemented.*

To keep the comparison fair, all three will share: the same fixed `BATCH` widths, the same
byte-array account layouts (§2.7), the same trivial `apply`, and this same mollusk harness —
so the table measures framework overhead, not consumer logic.

## Caveats

Single-shard, single-machine, demo `apply`. Not a throughput benchmark (see `SPEC.md` §7
for the sharded settle-time model). `ID` is a placeholder; figures are for relative
comparison, not a deployment guarantee.
