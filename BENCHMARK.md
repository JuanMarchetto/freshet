# freshet — compute-unit benchmark

A primitive that cranks state across many accounts lives or dies on its per-member
compute cost, so freshet treats CU as a first-class, measured property — not a claim.

> **Honesty up front.** All three frameworks — **Pinocchio**, **Anchor**, and **Quasar** —
> are implemented and measured below (real mollusk numbers, no estimates). All three share
> the same byte layout, the same trivial `apply`, the same fixed batch widths, and the same
> mollusk harness, so the table isolates framework cost, not consumer logic.

## Methodology

- Harness: [`mollusk-svm`](https://github.com/anza-xyz/mollusk) 0.13, single-instruction
  execution against the built `.so`, reading `InstructionResult::compute_units_consumed`.
- The reference consumer's `apply` is a trivial saturating add, so the reported figures
  are **freshet's own overhead** — per-member cost is dominated by membership verification
  (the canonical-PDA check), not domain logic. A real consumer's `apply`/`reduce` cost
  stacks on top of these.
- Reproduce:

  ```bash
  # Pinocchio + Anchor (root workspace)
  cargo build-sbf --manifest-path program/Cargo.toml
  SBF_OUT_DIR=target/deploy cargo test -p freshet-program --test bench_cu -- --nocapture
  cargo build-sbf --manifest-path anchor/Cargo.toml
  SBF_OUT_DIR=target/deploy cargo test -p freshet-anchor --test bench_cu -- --nocapture

  # Quasar (standalone workspace — its own Cargo.lock, see quasar/Cargo.toml)
  cd quasar && cargo build-sbf
  SBF_OUT_DIR=$(pwd)/target/deploy cargo test --test bench_cu -- --nocapture
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
| `advance_apply` overhead (intercept) | 3 844 CU | 5 222 CU | **942 CU** |
| `advance_apply` per-member (slope) | 1 683 CU | 1 897 CU | **391 CU** |
| `advance_apply` (batch = 2) | 7 212 CU | 9 016 CU | **1 716 CU** |
| `try_finish_apply` | 173 CU | 925 CU | 179 CU |
| `finalize` | 78 CU | 600 CU | 50 CU |

The surprise is Quasar: **~4× cheaper per member** than either alternative, and the lowest
fixed overhead of the three. The reason is concrete and reproducible — see below.

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

### Quasar (measured)

Quasar lands **well below** both other frameworks, not near Pinocchio as first expected.

`advance_apply` scaling (single shard):

| batch | CU |
|--:|--:|
| 1 | 1 369 |
| 2 | 1 716 |
| 4 | 2 452 |
| 8 | 4 092 |

Least-squares fit: **overhead ≈ 942 CU + ≈ 391 CU per member.**

The whole table is dominated by one design choice: **how each framework checks a PDA.**

- Anchor and the Pinocchio port both verify membership with `create_program_address`
  (the `sol_create_program_address` syscall, ~1 500 CU/call). With one such check per
  member, that syscall *is* the per-member slope (~1.7–1.9k CU).
- Quasar verifies the same canonical PDA with its own `verify_program_address`, which calls
  `sol_sha256` + `sol_curve_validate_point` directly (~544 CU). Same zero-trust guarantee —
  the supplied account must hash to its index — at roughly a third of the cost. That single
  primitive accounts for most of the 4× per-member gap (391 vs 1 683 CU).
- The low fixed overhead (942 CU) is Quasar's lean 1-byte-discriminator dispatch and
  zero-copy account parsing — no borsh, no 8-byte sighash, minimal `Context` construction.
  It even edges out the hand-rolled Pinocchio path on the tiny ops (`finalize` 50 vs 78 CU).

**Honest caveat on what this does and doesn't show.** This is a comparison of each framework
*as idiomatically written* — every port uses the PDA-verification helper its framework
ships. It is **not** a claim that Quasar is inherently 4× faster: a Pinocchio program could
hand-roll the same `sha256`+`curve_validate` check and close most of the gap. The result's
real lesson is that the *default* membership primitive dominates a permissionless crank's
hot path, and Quasar's default is the efficient one. Quasar is a beta git dependency
(`blueshift-gg/quasar`, not on crates.io, pinned to a commit) and its typed
`remaining_accounts` caps at 64, so the member batch rides in the raw remaining-accounts
region (see `SPEC.md` §15.1). The crate lives in a standalone workspace because Quasar's
solana-crate version requirements conflict with Anchor/mollusk's under one lockfile.

## Caveats

Single-shard, single-machine, demo `apply`. Not a throughput benchmark (see `SPEC.md` §7
for the sharded settle-time model). `ID` is a placeholder; figures are for relative
comparison, not a deployment guarantee.
