# freshet — compute-unit benchmark

A primitive that cranks state across many accounts lives or dies on its per-member
compute cost, so freshet treats CU as a first-class, measured property — not a claim.

> **Honesty up front.** **Pinocchio** and **Anchor** are implemented and measured below
> (real mollusk numbers). **Quasar** is not yet ported, so its column is left empty on
> purpose — no estimated figures. All three share the same byte layout, the same trivial
> `apply`, the same fixed batch widths, and the same mollusk harness, so the table
> measures framework overhead, not consumer logic.

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

## Cross-framework

The headline artifact: the *same primitive* (same byte layout, same validation, same
delegation to the verified `freshet::state` guards), in three frameworks, benchmarked
identically. Measured with `cargo test -p freshet-anchor --test bench_cu` etc.

| metric | Pinocchio | Anchor | Quasar |
|---|--:|--:|--:|
| `advance_apply` overhead (intercept) | **3 844 CU** | **5 222 CU** | — |
| `advance_apply` per-member (slope) | **1 683 CU** | **1 897 CU** | — |
| `advance_apply` (batch = 2) | 7 212 CU | 9 016 CU | — |
| `try_finish_apply` | 173 CU | 925 CU | — |
| `finalize` | 78 CU | 600 CU | — |

### Pinocchio vs Anchor (measured)

Both implementations do **identical work** — same byte-array layout, same owner/disc/
canonical-PDA checks, same escrow binding, same per-member loop, same `freshet` core
guards. The gap is purely Anchor's framework wrapper:

- **Fixed overhead: +1 378 CU** (3 844 → 5 222). This is Anchor's 8-byte sighash dispatch,
  borsh arg decode, and `Context`/`UncheckedAccount` construction. It shows starkly on the
  tiny ops — `finalize` 78 → 600 CU, `try_finish_apply` 173 → 925 CU — which are almost
  entirely framework overhead since they touch one account.
- **Per-member: +214 CU** (1 683 → 1 897), ~13% — Anchor's per-account handling on top of
  the shared canonical-PDA check that dominates either way.
- **Net at batch = 2: +25%** (7 212 → 9 016); the relative gap shrinks as batch grows
  because the fixed overhead amortizes (at batch = 8: 17 309 → 20 398, +18%).

Takeaway: Anchor costs a flat ~1.4k CU per instruction for its ergonomics; for a crank
invoked thousands of times the hand-rolled Pinocchio path is the better economic fit,
which is the case freshet's reference settler makes.

### Quasar (pending)

Zero-copy / `no_std` like Pinocchio; expected to land near the Pinocchio column. It is a
beta git dependency (`blueshift-gg/quasar`, not published to crates.io) and its typed
`remaining_accounts` caps at 64, so the member batch uses the raw accessor (see `SPEC.md`
§15.1). Port + measurement is the remaining work; **no number is asserted until it builds.**

## Caveats

Single-shard, single-machine, demo `apply`. Not a throughput benchmark (see `SPEC.md` §7
for the sharded settle-time model). `ID` is a placeholder; figures are for relative
comparison, not a deployment guarantee.
