# freshet — Formal Specification v0.6

> **Changelog v0.5 → v0.6** (after implementing the Pinocchio program + mollusk
> integration tests, 2026-06-16 — errata confirmed by real SBF compilation):
> - **§2.7 erratum:** on-chain layouts are LE byte-array fields with `align_of == 1`
>   (the §2 offsets are ≡2 mod 8, so native `#[repr(C)]` would repad); values are read by
>   COPY (`from_le_bytes`), never `&u64` into the buffer. "zero_copy parity" = identical
>   byte offsets via byte arrays. (`program/src/layout.rs` enforces this with const
>   `offset_of!`/`align_of` asserts.)
> - **§2.1/§12:** `Status` is `#[repr(u8)]` Pending=0..Cancelled=5 with checked
>   `TryFrom<u8>`; a forged byte ≥6 ⇒ new code **`BadStatus = 25`** (not WrongPhase).
> - **§1:** `Monoid` gains a no_std byte contract (`to_acc_bytes`/`from_acc_bytes` over the
>   64-byte window) so `IDENTITY` (de)serializes soundly (MaxWinner sentinel = u64::MAX).
> - **§6/§11:** authorizer "P" is realized as `is_signer()` (an `invoke_signed` PDA arrives
>   as a signer) — the instructions-sysvar path is dropped.
> - **§6.1:** keeper payout is a direct lamport swap between two consumer-owned accounts
>   (no System CPI); `AccountDataTooSmall = 24` for the member-body length precheck.
> - **Status:** core (partition/state/monoid) + the full 16-instruction Pinocchio program
>   are implemented; `cargo build-sbf` links; mollusk integration tests pass (full
>   single-pass lifecycle → Done; cross-escrow drain rejected). Deferred: System-CPI
>   account creation, pull mode (gated off, §15), the full T1–T11 matrix + CU gate.

# (historical) freshet — Formal Specification v0.5

> (Project renamed **Cascade → freshet** in v0.4: "cascade"/"cascada" were taken on
> crates.io. `freshet` = a sudden river surge fed by many tributaries — the fan-out
> metaphor; figuratively "a sudden overflow/abundance"; free on crates.io, no negative
> connotation / no software-crypto trademark collision. Crate = `freshet`; GitHub org
> `freshet-rs`; demo programs `freshet-royale` etc. The on-chain control account is
> still named `Effect`.)
>
> **Changelog v0.4 → v0.5** (after the state-machine model-check-lite, 2026-06-16, 86
> agents — 31-transition table verified; 2 HIGH lifecycle-guard gaps found):
> - **[HIGH]** `seal` asserts **`shards_created == shard_count`** (`ShardsIncomplete`):
>   `init_shards` (create branch) increments a new `Effect.shards_created` counter; else
>   `seal` could succeed with missing Shard PDAs and `advance_*` would hit a
>   non-existent shard.
> - **[HIGH]** `reset` asserts the effect was **previously sealed** ∧ `1 ≤ shard_count ≤
>   total` (`NotSealed`): a `cancel` from `Pending` (never sealed) then `reset` would
>   enter `Reducing`/`Applying` with an invalid/empty partition.
>
> **Changelog v0.3 → v0.4** (after the v0.3 re-verification red-team, 2026-06-16, 141
> agents — NO-freeze; findings now dominated by spec-internal contradictions, not
> architecture):
> - **[CRITICAL]** Lazy per-epoch reset prologue made **universal** — `skip`/`skip_reduce`
>   (not just `advance_*`) run it before advancing the cursor, else a skip as the first
>   touch of a stale shard after `reset` wedged the re-run.
> - **[HIGH]** `init_effect` sets **`epoch = 1`** (0 reserved as the never-applied
>   sentinel) — fixes first-epoch all-skip Done (`0 >= 0` gate) and the epoch-0 refund
>   lockout.
> - **[HIGH]** §9.4 rewritten to match §10 (no pull epoch gate) — removed the
>   self-contradiction that re-introduced the freeze bug.
> - **[HIGH]** `reset` clears the skip counters; `skipped_count` **split into
>   `reduce_skipped_count` / `apply_skipped_count`** (a REDUCE skip corrupts the
>   aggregate; an APPLY skip doesn't) — `assert_settled` requires `reduce_skipped == 0`.
> - **[HIGH]** **pull-XOR-push** exclusivity asserted at `init_effect`; `init_shards`
>   `IDENTITY` write scoped to the create branch only.
> - **[MED]** reset target pinned (`requires_reduce → Reducing`, else `Applying`); §8
>   "validated at seal" corrected to lazy per-crank enforcement.

# (historical title) Cascade — Formal Specification v0.3

> Authoritative spec. Supersedes the mechanism sketches in `DESIGN.md` wherever they
> differ. `DESIGN.md` remains the rationale/strategy doc; this is the contract.
> Status: pre-implementation. **Freeze gate: a formal model-check of §4 + §5 (§15.5)
> MUST pass before implementation freeze** — the re-verification showed model-checkable
> liveness wedges slip past human review.
>
> **Naming:** the published library crate is **`freshet`** (single crate; no `-core`
> suffix). Framework glue, if needed, ships as cargo features (`anchor`/`pinocchio`/
> `quasar`) within `freshet`, not as separate crates. Reference *programs* (the demo
> game, a market settler) are separate binaries (`freshet-royale`, …), not library
> crates.
>
> **Changelog v0.2 → v0.3** (after the v0.2 re-verification red-team of 2026-06-16,
> 117 agents: NO-freeze verdict, 1 critical + 6 high new/regressed):
> - **[CRITICAL fixed]** Completion is now **derived from `cursor == len`**, not a
>   separate `*_DONE` flag. `skip()` on the last member no longer wedges `Applying`
>   forever. The `REDUCE_DONE`/`APPLY_DONE`/`MERGED` flags are **removed** — this kills
>   the entire flag-vs-cursor divergence bug class.
> - **[HIGH fixed]** Pull drops the epoch gate (idempotent via `snapshot == acc_global
>   ⇒ owed = 0`); `reset` is **forbidden** on `pull_enabled` effects; `pull` uses
>   `saturating_sub`.
> - **[HIGH fixed]** Two-phase funding is **phase-aware (2×)** for `requires_reduce`.
> - **[HIGH fixed]** `reset` clears the `finalized` flag; `refund_escrow` **sweeps to
>   rent floor and never closes** (so re-runs aren't stranded) + carries an epoch stamp.
> - **[MED fixed]** `skip_reduce` added (REDUCE-phase poison escape); `acc_global` /
>   `acc_partial` **explicitly initialized to `IDENTITY`** at create (non-zero-identity
>   monoids); defense-in-depth guards (shard_id bound, status asserts, u64 arithmetic).
>
> **v0.1 → v0.2 (prior round, 153 agents):** escrow bound to `(effect,shard_id)`;
> `shards_done` de-hot-pathed (Effect read-only on cranks); computed partition (no
> fan-out); incremental `reduce_shards`/refunds; `reset→Applying` guard; all-or-nothing
> `pay_keeper`.

freshet is a **library** (crate / framework macro), not a standalone program, that
gives a consumer Solana program a **resumable, permissionless, shardable MapReduce
over an account set**: scan N member accounts to compute an aggregate (REDUCE), then
apply an effect to M member accounts (MAP), with both N and M unbounded by the
per-transaction account limit. Plus a **pull (lazy)** mode for formula-expressible
effects.

---

## 0. Why a library, not a program (load-bearing)

On Solana, **only the program that owns an account may mutate its data or debit its
lamports** (runtime-enforced). Therefore:

- A standalone freshet *program* could not write the consumer's game/market state; it
  would need a CPI callback per member, re-incurring the ≤16-account CPI limit and
  heavy CU. **Rejected.**
- freshet ships as a **crate linked into the consumer program**. The `Effect`,
  `Shard`, `Member`, and `Escrow` accounts are **owned by the consumer program**. The
  crate provides the harness (cursor, batching, PDA-by-index, idempotency, bounty,
  sharding, phases); the consumer implements the domain logic via a trait.

Consequence baked into every check below: **owner assertions are against the consumer
program ID**, which the crate is parameterized over (`CONSUMER_ID`).

---

## 1. The rule model — the consumer's contract

```rust
/// Implemented by the consumer. freshet drives it.
pub trait Effectful {
    type Member: Pod;            // consumer state at member_pda(effect, i)
    type Params: Pod;            // rule inputs, stored in the Effect (≤128 B)
    type Acc: Monoid;            // accumulator value (≤64 B inline; see §2.5 for larger)

    const FLAGS: RuleFlags;      // requires_reduce | order_independent | pull_enabled

    /// REDUCE: fold one member into the accumulator. MUST NOT mutate the member.
    fn reduce(acc: &mut Self::Acc, m: &Self::Member, index: u64, p: &Self::Params);

    /// MAP: apply the FINAL accumulator + params to one member. Mutates the member.
    /// MUST read/write ONLY this member (no cross-member reads — see §9).
    fn apply(m: &mut Self::Member, acc: &Self::Acc, index: u64, p: &Self::Params)
        -> Result<(), ApplyError>;

    /// PULL (optional): lazily compute this member's update on next touch.
    /// MUST be CUMULATIVE in `acc` (computes an absolute target via SATURATING
    /// subtraction from the running aggregate, not a per-epoch delta) so a member that
    /// skips epochs or re-pulls within an epoch still converges (re-pull ⇒ owed 0).
    /// Cumulative-ness is an UNENFORCEABLE consumer contract (like the monoid laws);
    /// §14 mandates a skipped-epoch convergence property test. See §10.
    fn pull(m: &mut Self::Member, acc: &Self::Acc, p: &Self::Params)
        -> Result<(), ApplyError>;
}

/// Accumulator MUST be a commutative monoid so shards can be merged in any order.
pub trait Monoid: Pod + Copy {
    const IDENTITY: Self;
    fn combine(a: Self, b: Self) -> Self;   // associative AND commutative
}
```

**Why the monoid constraint:** sharded REDUCE produces one partial accumulator per
shard; `reduce_shards` folds them (§6.5). The fold order is **pinned ascending by
`shard_id`**, so *associativity* alone suffices for determinism — but commutativity is
still required as defense against an accidentally order-sensitive `combine`. Rules
whose aggregate is not a commutative monoid (e.g. "the median", order-dependent folds)
**cannot be sharded**: they MUST set `shard_count = 1`. `shard_count > 1` is permitted
only if `FLAGS.order_independent` is set, asserted at `init_effect` (else `NotShardable`).

> Monoid laws are an **unenforceable consumer contract** (as in every monoid library).
> §14 mandates a property-test obligation (associativity / commutativity / identity)
> in the benchmark harness. Floating point is forbidden in `Acc` (non-associative);
> `Acc` must be integer/bytewise `Pod`. For bounded top-k / histograms, the partial
> must retain the *full* top-k for `combine` to stay associative.

**Single-pass fast path:** if `FLAGS.requires_reduce == false` (the effect does not
depend on a global aggregate — e.g. "add 10 HP to everyone"), the REDUCE phase is
skipped and `apply` is called with `acc = Acc::IDENTITY`.

---

## 2. Account model & byte layouts

All accounts are PDAs owned by `CONSUMER_ID`. All carry an 8-byte type discriminator
(prevents type cosplay) and a `version` byte (see §2.6 versioning). Multi-byte
integers are little-endian, `#[repr(C)]`. See §2.7 on alignment/zero-copy.

### 2.1 `Effect` — the control account
```
offset field          type      notes
0      disc           [u8;8]    = DISC_EFFECT
8      version        u8        = 2
9      status         u8        Status enum (§5); checked conversion, never raw match
10     bump           u8        canonical bump of this PDA
11     flags          u8        sharded|requires_reduce|order_independent|pull|acc_external|finalized
12     rule_id        u16
14     shard_count    u32       P (≥ 1)
18     shards_done    u32       APPLY-phase ordered completion cursor; written ONLY by try_finish_apply
22     merge_cursor   u32       REDUCE-phase ordered merge cursor; written by reduce_shards / reset / begin_apply
26     epoch          u64       **= 1 at init_effect** (0 reserved as never-applied/never-merged sentinel); bumped on reset
34     total          u64       frozen at seal()
42     reduce_skipped_count u64 members skipped in REDUCE (CORRUPTS acc_global); cleared on reset
50     authority      Pubkey    wallet OR program/PDA (see §6 authorizer matrix)
82     acc_global     [u8;64]   merged accumulator after reduce_shards (or AccExt key if acc_external)
146    params         [u8;128]  rule params
274    created_slot   u64       observability only (Clock at init)
282    apply_skipped_count u64  members skipped in APPLY (benign per-member); cleared on reset
290    shards_created u32       count of Shard PDAs created by init_shards (create branch); seal asserts == shard_count
294    _reserved      [u8;2]
TOTAL  296 bytes
```
> `epoch` starts at **1**, never 0: member `last_*_epoch` and `shard.epoch` are zeroed at
> account creation, so a 0 effect-epoch would make the `last_*_epoch < epoch` gate read
> `0 < 0 == false` and silently skip every member to a vacuous `Done`. `seal` asserts
> `epoch >= 1`. **`shards_done` (L18) is written by `try_finish_apply`/`begin_apply`/
> `reset`; `merge_cursor` (L22) by `reduce_shards`/`begin_apply`/`reset`.**
```
> (layout note continues — original `acc_global`/`params` external caveat below)
```
`start/len` are **NOT stored** — every instruction computes its shard's range
deterministically (§2.8). This makes `seal`/`reset` Effect-only writes (no P-fan-out).

### 2.2 `Shard` — one per partition (lazy per-epoch reset)
```
0   disc          [u8;8]   = DISC_SHARD
8   version       u8
9   bump          u8
10  _pad          [u8;2]
12  shard_id      u32      p ∈ [0, P)
16  effect        Pubkey
48  epoch         u64      effect-epoch these cursors are valid for (lazy reset, §5)
56  reduce_cursor u64      local; ∈ [0, len]
64  apply_cursor  u64      local; ∈ [0, len]
72  acc_partial   [u8;64]  this shard's REDUCE result (= IDENTITY at create / first touch)
136 _reserved     [u8;16]
TOTAL 152 bytes
```
**v0.3: no `*_DONE`/`merged` flags.** Phase completion is derived: a shard is *done*
for the current phase iff `shard.epoch == effect.epoch ∧ phase_cursor == len`. This
removes the flag-vs-cursor divergence that let `skip()` wedge `Applying` in v0.2. A
stale shard (`shard.epoch < effect.epoch`) is never *done* — it must be cranked (which
lazily resets it). `reduce_shards` ordering (`shard_id == merge_cursor`) provides
merge-exactly-once without a per-shard `merged` flag.

### 2.3 `Escrow` — one per shard (lamport bounty, isolated from state)
```
0   disc              [u8;8]  = DISC_ESCROW
8   version           u8
9   bump              u8
10  effect            Pubkey
42  shard_id          u32
46  bounty_per        u64     paid PER PROCESSED ITEM (never per batch); immutable after init
54  last_refund_epoch u64     epoch of last refund_escrow (prevents double/cross-epoch refund)
62  _reserved         [u8;2]
TOTAL 64 bytes
```
Lamports above `rent_exempt(64)` are the spendable bounty. **State lives in `Shard`;
lamports live in `Escrow`** — paying a keeper never risks a state account's rent floor.
**v0.3: `refund_escrow` sweeps surplus down to the rent floor and NEVER closes the
account** (closing stranded re-runs in v0.2 — `reset` had no recreation path). `top_up_
bounty` refills it for a re-run. `bounty_per` is **immutable** after `init_shards`.

### 2.4 `Member` — consumer state + freshet header
```
0   disc              [u8;8]   = DISC_MEMBER
8   version           u8
9   bump              u8       canonical bump of THIS member PDA
10  effect            Pubkey
42  index             u64      GLOBAL index (immutable after enroll)
50  last_reduce_epoch u64
58  last_apply_epoch  u64
66  <consumer Member: Pod follows here>
```

### 2.5 `AccExt` (optional, `flags.acc_external`) — large accumulator
A reallocatable PDA at `accext` (§3) holding an `Acc` larger than 64 B (top-k,
histogram, Merkle root set). When `acc_external`, `Effect.acc_global[0..32]` holds the
`AccExt` pubkey and `advance_*`/`reduce_shards` take `AccExt` in their account list.
**v0.1 scope decision:** `acc_external` is *specified but OPTIONAL for the first
implementation*; v0.2 reference impl MAY ship inline-only (≤64 B) and gate
`acc_external` off. See §15.

### 2.6 Versioning
Every load asserts `version <= SUPPORTED_VERSION`; a higher version hard-fails
(`BadVersion`). An authority-gated `migrate` instruction (out of scope for v0.2,
reserved) performs in-place `realloc`+rewrite. Until then `version` is a forward-compat
reject guard, not a live migration path.

### 2.7 Alignment / zero-copy
Layouts are `#[repr(C)]` with every multi-byte field **naturally aligned** (the offset
tables above are aligned: u64s at 8-aligned offsets, Pubkeys at their boundaries),
enabling true zero-copy (`bytemuck`/Anchor `zero_copy` parity) without `packed`
unaligned reads. The consumer `Member` body MUST begin at an 8-aligned offset (header
is 66 B → pad to 72 if the body needs 8-alignment; the macro inserts this pad).
`.cast::<T>()` is defined as a size-checked `bytemuck::from_bytes` (compile/load-time
length assert).

### 2.8 Deterministic partition (computed, never stored)
```
base = total / P ;  rem = total % P
len(p)   = base + (p < rem ? 1 : 0)
start(p) = p * base + min(p, rem)
```
`seal` asserts `P ≤ total` (and `total ≥ 1`, `P ≥ 1`), so **every shard has len ≥ 1**
— no empty-shard wedge. Any instruction recomputes `len(shard_id)` from
`(Effect.total, Effect.shard_count)`.

---

## 3. PDA derivation (all under `CONSUMER_ID`)

```
effect      : consumer-chosen seeds, e.g. [b"effect", game, round.le()]
shard(p)    : [b"cas.s", effect, p.le()]
escrow(p)   : [b"cas.e", effect, p.le()]
member(i)   : [b"cas.m", effect, i.le()]          // i = GLOBAL index
accext      : [b"cas.a", effect]
```
Canonical bump found with `find_program_address` **at creation only**; stored in the
account header. Later verification uses `create_program_address(seeds, stored_bump)`
and asserts equality with the passed key (cheap). Canonicality is guaranteed
transitively: an account could only have been created by `enroll`/`init_*`, which use
the canonical bump and assert the slot empty (§6) — so no non-canonical twin can occupy
a slot.

---

## 4. Idempotency & exactly-once (proof obligation)

**Within an epoch + phase**, exactly-once follows from three enforced facts:
1. `advance_*` **derives** the processed range from the on-chain cursor; it accepts
   **no index and no `filled`** from the caller. `n = min(BATCH, len - cursor)`,
   computed on-chain (`len` from §2.8).
2. For slot `j ∈ [0, n)` the handler asserts the passed account's header satisfies
   `header.effect == effect ∧ header.index == start + cursor + j` and key-derivation
   consistency (§3). The caller cannot substitute, reorder, duplicate, or skip.
3. `cursor += n` and all member writes commit in **one transaction** (Solana atomic).
   A failed batch reverts wholesale; cursor never advances past unprocessed members.
   *Reorg safety follows from (3): a fork rollback reverts cursor, member writes, and
   the epoch stamp together; the canonical fork re-applies once from un-mutated state.*

**Across epochs** (after `reset`): cursor monotonicity is insufficient (a reset
re-walks `[0,len)`). Each member stores `last_apply_epoch` / `last_reduce_epoch`;
`apply`/`reduce` is gated `member.last_*_epoch < effect.epoch` and stamps it to
`effect.epoch`. Idempotent per epoch; defeats cross-epoch double-apply.

**reset guards (v0.2 + v0.3):**
- `reset` target is **pinned** (v0.4, no disjunction — required for the §15.5
  model-check): `requires_reduce → Reducing`; single-pass → `Applying`. Never `Pending`
  (re-entering `Pending` would reopen enrollment and shift the partition). A
  `requires_reduce` re-run recomputes `acc_global` via a fresh REDUCE; single-pass
  re-applies against `acc_global == IDENTITY`.
- **(v0.3)** `reset` MUST clear the `finalized` flag (a re-run that can't be finalized
  is a permanent lockout) and reset `shards_done = 0`, `merge_cursor = 0`,
  `acc_global = IDENTITY`.
- **(v0.3)** `reset` is **FORBIDDEN on `pull_enabled` effects** (`PullNoReset`): zeroing
  `acc_global` while members hold stale snapshots silently under-pays / underflows.
  Rotate reward rounds via a fresh `Effect`, never `reset`.

---

## 5. State machine

```
Pending ──seal──▶ (requires_reduce ? Reducing : Applying)        # Effect-only write

Reducing ──[advance_reduce × shards]──▶ shard reduce_cursor→len (Shard-only write)
Reducing ──[reduce_shards, incremental, ascending]──▶ merge_cursor==P ⇒ Reduced
Reduced  ──begin_apply──▶ Applying                               # Effect-only: shards_done=0

Applying ──[advance_apply × shards]──▶ shard apply_cursor→len (Shard-only write)
Applying ──[try_finish_apply, incremental, ascending]──▶ shards_done==P ⇒ Done

Done     ──finalize──▶ Done (sets finalized flag, emits)         # Effect-only write
Done/Cancelled ──[refund_escrow(p)]──▶ sweep to rent floor (NEVER close); stamp epoch
any(≠Done/Cancelled) ──cancel──▶ Cancelled                       # authority
Done/Cancelled ──reset──▶ (requires_reduce ? Reducing : Applying)   # pinned, never Pending
       # authority; epoch+=1; clears finalized + reduce_skipped + apply_skipped; acc_global=IDENTITY
       # FORBIDDEN if pull_enabled (PullNoReset)
```
**Completion is cursor-derived (v0.3):** a shard is *done* for a phase iff
`shard.epoch == effect.epoch ∧ phase_cursor == len(shard_id)`. No `*_DONE` flag exists,
so `skip()` (which advances the cursor) can never leave a shard "at len but not done".

**De-hot-pathed phase promotion (the v0.2 parallelism fix):** `advance_reduce` and
`advance_apply` write **only their `Shard`** (and pay from their `Escrow`); the
`Effect` is passed **read-only**. So Sealevel co-schedules up to P cranks per block
(they write-conflict only on distinct Shard/Escrow accounts). Phase promotion is moved
to dedicated, infrequent ops (`reduce_shards`, `try_finish_apply`) that run
`ceil(P / batch)` times total and serialize on the `Effect` — negligible beside the
`total/BATCH` crank operations.

**Lazy per-epoch reset (UNIVERSAL — v0.4):** a `Shard` carries `epoch`. **Every
Shard-mutating instruction** — `advance_reduce`, `advance_apply`, **`skip`, and
`skip_reduce`** — runs the identical prologue *before* advancing any cursor:
`if shard.epoch < effect.epoch { reduce_cursor = apply_cursor = 0; acc_partial = IDENTITY;
shard.epoch = effect.epoch }`. (v0.3 omitted it from `skip`/`skip_reduce`, so a skip as
the first touch of a stale shard advanced a stale cursor and wedged the re-run.) The
read-only promotion ops (`reduce_shards`/`try_finish_apply`) do **not** lazy-reset; they
hard-assert `shard.epoch == effect.epoch` and treat a stale shard as not-done. So `reset`
only writes `Effect`
(bumps `epoch`; resets `shards_done`, `merge_cursor`, `acc_global=IDENTITY`; clears
`finalized`) and shards reset themselves on next touch — no P-account fan-out. A stale
shard (`shard.epoch < effect.epoch`) is **never counted done/merged** (promotion ops
assert `shard.epoch == effect.epoch`), so it cannot prematurely complete a phase after
reset.

---

## 6. Instructions

Authorizer matrix: **W** = `authority` must be a tx signer (wallet); **P** = PDA
authority authorizes by being the CPI caller (verified via instructions sysvar);
permissionless = anyone.

| ix | authz | summary |
|---|---|---|
| `init_effect` | W/P | create `Effect` (Pending); **set `epoch = 1`**, `acc_global = IDENTITY`; set rule/params/flags/shard_count/authority; assert `shard_count ≥ 1` ∧ (`shard_count==1` ∨ `order_independent`) ∧ **`!acc_external`** (deferred, §2.5). **pull-XOR-push:** if `pull_enabled`, push instructions (`seal`/`advance_*`/`begin_apply`) reject with `PullPushExclusive` |
| `init_shards(range)` | W/P | create a bounded batch of `Shard`+`Escrow` PDAs; fund escrows. **Create branch ONLY: set `acc_partial = IDENTITY`, cursors/epoch = 0, `shards_created += 1`.** Existing-valid → **fund-only (additive), NEVER touch acc_partial/cursors/epoch/shards_created**; forbidden once `status != Pending`. No start/len (computed) |
| `enroll` / `enroll_batch(k)` | W/P | create member PDA(s) at `total..total+k`; set header; `total += k`. `k ≤ ENROLL_MAX` (§6.10) |
| `seal` | W/P | assert `1 ≤ shard_count ≤ total` ∧ `epoch ≥ 1` ∧ `!pull_enabled` ∧ **`shards_created == shard_count`** (`ShardsIncomplete`); freeze `total`; `Pending → Reducing/Applying`. Effect-only |
| `advance_reduce(shard, escrow, [BATCH members])` | permissionless | REDUCE fold; pay per item. Shard+Escrow write, **Effect read-only**. Done when `reduce_cursor==len` |
| `reduce_shards([≤CAP shards])` | permissionless | incremental ascending merge `acc_partial→acc_global`; `merge_cursor==P ⇒ Reduced` |
| `begin_apply` | permissionless | assert `Reduced`; `→ Applying`; `shards_done=0`. Effect-only |
| `advance_apply(shard, escrow, [BATCH members])` | permissionless | MAP apply; pay per item. Shard+Escrow write, **Effect read-only**. Done when `apply_cursor==len` |
| `try_finish_apply([≤CAP shards])` | permissionless | assert `Applying`; incremental ascending; shard done iff `epoch==e.epoch ∧ apply_cursor==len`; `shards_done==P ⇒ Done` |
| `finalize` | permissionless | assert `Done ∧ !finalized`; set `finalized`; emit. Effect-only |
| `refund_escrow(shard)` | permissionless | from `Done`/`Cancelled` ∧ `last_refund_epoch < epoch`: sweep surplus to authority **down to rent floor (NEVER close)**; stamp `last_refund_epoch`. Per-shard |
| `top_up_bounty(shard)` | permissionless | add lamports to an escrow (anti-stall / re-fund a re-run) |
| `skip(shard, member)` | W/P | APPLY poison escape (runs §5 lazy-reset prologue + `assert Applying ∧ apply_cursor<len` first): `apply_cursor+=1`, stamp `last_apply_epoch`, `apply_skipped_count+=1`, emit `Skipped{phase:Apply}` |
| `skip_reduce(shard, member)` | W/P | REDUCE poison escape (prologue + `assert Reducing ∧ reduce_cursor<len` first): `reduce_cursor+=1`, stamp `last_reduce_epoch`, `reduce_skipped_count+=1`, emit `Skipped{phase:Reduce}`. Member dropped from `acc_global` |
| `cancel` | W/P | `→ Cancelled` (then `refund_escrow` per shard) |
| `reset` | W/P | **forbidden if `pull_enabled` (`PullNoReset`)**; assert **previously sealed ∧ `1 ≤ shard_count ≤ total`** (`NotSealed` — closes the cancel-before-seal wedge); `epoch += 1`; `shards_done=0`, `merge_cursor=0`, `acc_global=IDENTITY`, **clear `finalized`, `reduce_skipped_count=0`, `apply_skipped_count=0`**; status per §4 guard (`requires_reduce → Reducing`, else `Applying`) |

### 6.1 `advance_apply` — the hot path (canonical pseudocode)
```
fn advance_apply(ctx, shard_acc, escrow_acc, member_accs[BATCH]):
    # --- Effect: READ-ONLY (no Effect write on the hot path) ---
    let e = load::<Effect>(ctx.effect)?
    assert e.owner == CONSUMER_ID && e.disc == DISC_EFFECT
    assert ctx.effect.key == create_pda(<effect seeds>, e.bump)        # consumer-defined seeds
    assert e.status == Applying                                        # else WrongPhase

    # --- Shard: write-locked, lazy per-epoch reset ---
    let s = load_mut::<Shard>(shard_acc)?
    assert s.owner == CONSUMER_ID && s.disc == DISC_SHARD
    assert s.effect == ctx.effect.key && s.shard_id < e.shard_count    # shard_id bound
    assert shard_acc.key == create_pda([b"cas.s", e.key, s.shard_id.le()], s.bump)
    if s.epoch < e.epoch: s.{reduce_cursor,apply_cursor}=0; s.acc_partial=IDENTITY; s.epoch=e.epoch  # lazy reset
    let len   = partition_len(e.total, e.shard_count, s.shard_id)      # §2.8, all u64
    let start = partition_start(e.total, e.shard_count, s.shard_id)
    assert s.apply_cursor < len                                        # else ShardComplete

    # --- Escrow: BOUND to (effect, shard_id) — CRITICAL (v0.2 fix) ---
    let esc = load::<Escrow>(escrow_acc)?
    assert esc.owner == CONSUMER_ID && esc.disc == DISC_ESCROW
    assert esc.effect == e.key && esc.shard_id == s.shard_id
    assert escrow_acc.key == create_pda([b"cas.e", e.key, s.shard_id.le()], esc.bump)

    let n = min(BATCH, len - s.apply_cursor)                           # derived, NOT from caller

    # --- all-or-nothing payment FIRST: if escrow can't cover, revert (no cursor move) ---
    let floor = rent_exempt(ESCROW_LEN)
    let amount = n * esc.bounty_per
    require(escrow_acc.lamports.checked_sub(floor)? >= amount, BountyUnderfunded)   # code 12

    for j in 0..n:
        let gi = start + s.apply_cursor + j
        let m  = member_accs[j]
        assert m.owner == CONSUMER_ID
        let h  = load_mut::<MemberHeader>(m)?
        assert h.disc == DISC_MEMBER
        assert h.effect == e.key && h.index == gi
        assert m.key == create_pda([b"cas.m", e.key, gi.le()], h.bump)
        if h.last_apply_epoch >= e.epoch: continue                     # cross-epoch idempotency
        C::apply(member_body_mut::<C::Member>(m), &acc_of(e), gi, &e.params.cast())?  # all-or-nothing
        h.last_apply_epoch = e.epoch
    # tail slots j ∈ [n, BATCH): MUST be a readonly sentinel (== effect key); checked, never loaded

    invoke_signed transfer amount: escrow_acc → ctx.cranker            # PDA-signed
    s.apply_cursor += n                                                # Shard write only; NO Effect write
    # completion is cursor-derived: try_finish_apply later sees apply_cursor==len ∧ epoch match
    emit Advanced{ effect:e.key, epoch:e.epoch, shard:s.shard_id, phase:Apply, to:s.apply_cursor, n }
```
`acc_of(e)` = `e.acc_global.cast()` inline (v0.3 reference impl is inline-only;
`acc_external` is asserted off at `init_effect` until that path ships — §2.5/§15.3).
`advance_reduce` is identical except: `status == Reducing`; members loaded
**read-only**; `C::reduce` folds into `s.acc_partial`; gates/stamps `last_reduce_epoch`;
advances `reduce_cursor`. No `*_DONE` flag is written by either (completion = cursor).

### 6.5 `reduce_shards` — incremental ascending merge (resumable)
```
fn reduce_shards(ctx, shard_accs[≤CAP]):                # CAP per framework (§6.10); ascending shard_id
    let e = load_mut::<Effect>(ctx.effect)?; assert e.status == Reducing
    for s_acc in shard_accs:
        let s = load::<Shard>(s_acc)?
        assert s.effect == e.key && s_acc.key == create_pda([b"cas.s", e.key, s.shard_id.le()], s.bump)
        assert s.shard_id == e.merge_cursor             # ascending order ⇒ exactly-once merge (no flag)
        assert s.epoch == e.epoch && s.reduce_cursor == partition_len(e.total,e.shard_count,s.shard_id)
        e.acc_global = combine(e.acc_global.cast(), s.acc_partial.cast())   # ascending ⇒ associative ok
        e.merge_cursor += 1
    if e.merge_cursor == e.shard_count: e.status = Reduced
```
`try_finish_apply([≤CAP shards])`: `assert e.status == Applying`; walk shards ascending
from `e.shards_done`, asserting `s.shard_id == e.shards_done ∧ s.epoch == e.epoch ∧
s.apply_cursor == partition_len(...)`; `e.shards_done += 1`; at `== shard_count` →
`Done`. Both ops derive completion from the cursor — **no `*_DONE` flag** — so a
`skip`-advanced cursor is counted identically to a cranked one (fixes the v0.2 wedge).

### 6.9 `skip` / `skip_reduce` (authority escape for a poison member)
Authority-gated. For the member at the current phase cursor (one that fails repeatably,
e.g. a panicking `C::apply` / `C::reduce`). **Both ops MUST run the universal lazy-reset
prologue (§5) and a phase + bound assert FIRST** (v0.4 — a `skip` as the first touch of
a stale shard after `reset` otherwise advances a stale cursor and permanently wedges the
re-run):
```
prologue: if s.epoch < e.epoch { s.reduce_cursor=s.apply_cursor=0; s.acc_partial=IDENTITY; s.epoch=e.epoch }
skip(shard, member):        assert e.status==Applying; assert s.apply_cursor < len(s.shard_id)
   s.apply_cursor += 1; stamp h.last_apply_epoch=e.epoch; e.apply_skipped_count += 1
   emit Skipped{index, phase:Apply}
skip_reduce(shard, member): assert e.status==Reducing; assert s.reduce_cursor < len(s.shard_id)
   s.reduce_cursor += 1; stamp h.last_reduce_epoch=e.epoch; e.reduce_skipped_count += 1   # member dropped from acc_global
   emit Skipped{index, phase:Reduce}
```
Because completion is **cursor-derived** (§5/§6.5), advancing the cursor to `len` via
`skip` is counted done by `try_finish_apply`/`reduce_shards` — **no wedge**. **Breaks
exactly-once loudly:** `assert_settled` requires `reduce_skipped_count == 0` (a REDUCE
skip corrupts the aggregate) and surfaces `apply_skipped_count` (per-member-benign). Both
counters are **cleared on `reset`**, where skipped members are re-attempted.

### 6.10 limits
`ENROLL_MAX` and `CAP` (for `reduce_shards`/`try_finish_apply`/`refund` batches) ≈
`128 − (Effect + payer + system/sysvars)` ≈ **123 / 120** accounts per tx. Stated
explicitly so account structs are sizeable.

---

## 7. Sharding & throughput

With the v0.2 de-hot-path (§5), `advance_*` write-locks only its `Shard`+`Escrow` and
reads `Effect`; **P cranks land per block** (Sealevel parallel). Phase-promotion ops
serialize on `Effect` but run only `ceil(P/CAP)` times.

**Settle time ≈ `total / (P · BATCH) / land_rate / slots_per_sec`** (≈2.5 slots/s).

| members | P=1, BATCH=16 (ideal) | realistic (land 0.5) | P for ≤60s |
|---|---|---|---|
| 10k | ~250 s | ~8 min | ~3 |
| 100k | ~42 min | ~80 min | ~33 |
| 1M | ~7 h | ~14 h | ~330* |

\* Block-limited: per-write-account CU cap (~12M) and block CU (~48M). At ~200k
CU/`advance`, ≈240 cranks/block ceiling regardless of P — beyond that, P buys
*pipelining depth across blocks*. **Sharding is mandatory above ~25–50k members for
latency-sensitive effects.** Size P to the latency target. Note `reduce_shards`/
`try_finish_apply`/`refund_escrow` each take `ceil(P/CAP)` txs, so very large P (e.g.
330) costs ~3 extra promotion txs per phase — negligible.

**Hard constraints for `shard_count > 1`** (else `shard_count = 1`, `NotShardable`):
- `Acc` is a commutative monoid (§1); `FLAGS.order_independent` set (asserted at init).
- No cross-member reads in `apply` (§9).

---

## 8. Keeper economics

- **Per-item payment only** (`n * bounty_per`); per-batch payment is forbidden
  (`filled=1` grind). `n` is on-chain-derived.
- **All-or-nothing (v0.2):** if `escrow.lamports − floor < n * bounty_per`, the crank
  **reverts** `BountyUnderfunded(12)` — the cursor does NOT advance and the batch waits
  for `top_up_bounty`. No work is ever left uncompensated behind the cursor.
- **Escrow isolated from state** (§2.3): payouts never breach a state account's rent
  floor; once the escrow binding (§6.1) holds, the constant `rent_exempt(64)` floor is
  exact.
- **Bounty sizing — phase-aware (v0.3); enforced lazily, NOT at `seal`** (v0.4 — `seal`
  is Effect-only and structurally cannot read the P escrow accounts; the same correction
  already applies to `reset`). It is an **off-chain funder obligation** + a §14 property
  target, with the per-shard per-crank `BountyUnderfunded(12)` gate as the real on-chain
  enforcement (a drained shard stalls loudly, recoverable via `top_up_bounty`):
  `escrow_balance ≥ rent_floor + PHASES · ceil(len/BATCH)·BATCH·bounty_per`, where
  **`PHASES = 2` for `requires_reduce` effects** (REDUCE pass + APPLY pass both pay per
  item from the same escrow) and `1` for single-pass. The v0.2 single-`len` formula
  deterministically stalled APPLY of every two-phase effect (REDUCE drained the
  escrow). `bounty_per ≥ p95(cost_to_land)` where `cost_to_land = base_fee +
  priority_fee_p95 + BATCH·per_member_CU·cu_price`.
- **Re-run funding is lazy, not checked at `reset`:** `reset` is Effect-only, so it does
  NOT read escrows. A drained re-run is caught by the all-or-nothing `BountyUnderfunded`
  gate on the first crank; `top_up_bounty` refills. (v0.2 wrongly implied `reset`
  performs per-shard escrow validation.)
- **Races:** honest crankers build identical txs; only one lands per shard/slot, the
  rest fail the cursor assert early (<1k CU). The real fix is **disjoint shard
  assignment to keepers** (off-chain / Tuk Tuk task partitioning).
- **Anti-stall:** `top_up_bounty` is permissionless (a winner can fund their own
  settlement). For forced-settlement effects the consumer SHOULD fund the escrow at
  `seal` from the settled pot and **MUST** designate a keeper-of-last-resort.
- **Tuk Tuk:** register `advance_*` per shard as tasks; bounty covers the fee. freshet
  = settlement; Tuk Tuk = permissionless transport.

---

## 9. Atomicity contract (the sharp edge)

freshet is **not atomic across batches**. Consumers MUST honor:

1. **`InProgress` is a valid, observable state.** Logic depending on the completed
   effect MUST load `Effect` and assert `status == Done ∧ epoch == expected_epoch ∧
   reduce_skipped_count == 0 ∧ apply_skipped_count == expected` via
   `assert_settled(effect, expected_epoch)`. The epoch check prevents reading a re-run's
   partial state as prior. A **REDUCE skip corrupts `acc_global`** (member dropped from
   the aggregate) ⇒ `reduce_skipped_count` must be 0 by default; an APPLY skip is
   per-member-benign (surfaced via `apply_skipped_count`). Both counters are **cleared on
   `reset`** (else a clean re-run is flagged "Done with N unapplied" forever).
2. **Single-pass `apply` may read/write ONLY the member at `cursor+j`.** Cross-member
   reads are forbidden (order-dependent → un-shardable). The fixed-width batch struct
   passes only the cursor-range accounts, enforcing this **structurally** — a guarantee.
3. **Effects with member interaction** (i depends on j's pre-effect state) MUST use
   two-phase: REDUCE snapshots the aggregate, MAP applies against the frozen
   `acc_global`. Never single-pass.
4. **Pull mode** is idempotent via the cumulative fixpoint
   `owed = acc_global.saturating_sub(snapshot)·stake; snapshot = acc_global` — **no epoch
   gate** (§10 is the single normative source). Pull effects are **pull-only**: push
   instructions (`seal`/`advance_*`/`begin_apply`) assert `!pull_enabled` (`PullPushExclusive`)
   so the two ledgers can't double-pay over the same frozen `acc_global`.

---

## 10. Pull (lazy) mode

For formula effects (`FLAGS.pull_enabled`): the event writes only `acc_global` +
`params` to the `Effect` (O(1)); no crank. On a member's next touch, the consumer calls
`C::pull(member, acc_of(effect), params)`.

**No epoch gate on pull (v0.3 fix).** v0.2's monotone gate
(`last_apply_epoch < epoch`) was wrong here: a reward event writes only `acc_global`
(never bumps `epoch` — only `reset` does), so the gate froze every member at its first
claim for the epoch's life. Instead, pull relies on the **cumulative fixpoint**:
`owed = acc_global.saturating_sub(member.snapshot) · stake`, then
`member.snapshot = acc_global`. A re-pull yields `owed = 0` (idempotent); a claimer that
skipped accruals converges in one touch. `saturating_sub` guarantees no underflow/wrap
even if a baseline ever moves backward.

**Soundness requirements (unenforceable consumer contract, like the monoid laws):**
1. `acc` is **cumulative/monotone** over the snapshot horizon (absolute target, not a
   per-epoch delta). §14 mandates a skipped-epoch convergence property test
   (one pull after k accruals == sum of k per-step pulls).
2. **`reset` is forbidden on `pull_enabled` effects** (§4, `PullNoReset`): zeroing
   `acc_global` while members hold stale snapshots silently under-pays (and would
   underflow without `saturating_sub`). Rotate reward rounds via a **fresh `Effect`**,
   not `reset`.

Pull cannot express forced effects (eliminations, world ticks) — those require push.

---

## 11. Security checklist (verify per framework: Quasar / Pinocchio / Anchor)

- [ ] **Signer/authorizer** per §6 matrix; `seal`/`enroll`/`skip`/`cancel`/`reset`
      authority-gated (closes premature-seal & total-inflation griefs).
- [ ] **Owner == CONSUMER_ID** on every Effect/Shard/Escrow/Member/AccExt load.
- [ ] **Discriminator** checked on every account (no cosplay across types).
- [ ] **Canonical bump** stored at create; verified via `create_program_address`.
- [ ] **Effect derivation** verified inline in `advance_*` (consumer seeds + bump).
- [ ] **Shard bound:** `s.effect==effect ∧ key==create_pda([b"cas.s",effect,p.le()],bump)`.
- [ ] **Escrow bound to shard (CRITICAL):** `esc.effect==effect ∧ esc.shard_id==s.shard_id
      ∧ key==create_pda([b"cas.e",effect,p.le()],bump)` **before** reading `bounty_per`.
- [ ] **`enroll` asserts slot empty** (lamports 0 / data 0) ∧ `index == total`.
- [ ] **No caller-supplied index or `filled`** in `advance_*`.
- [ ] **Effect read-only** in `advance_*` (parallelism + no hot-path write).
- [ ] **All-or-nothing batch**: any `apply` error reverts the tx; poison escape only via
      authority `skip` (loud, bumps `skipped_count`).
- [ ] **All-or-nothing pay**: revert `BountyUnderfunded(12)` if escrow can't cover `n`.
- [ ] **Universal lazy-reset prologue** (v0.4): `advance_reduce`, `advance_apply`,
      `skip`, `skip_reduce` ALL run `if s.epoch<e.epoch {…reset…; s.epoch=e.epoch}` before
      advancing any cursor; `skip`/`skip_reduce` also assert phase + `cursor<len`.
- [ ] **`epoch` initialized to 1** (`init_effect`); `seal` asserts `epoch≥1` (else the
      `last_*_epoch < epoch` gate at epoch 0 skips every member to a vacuous `Done`).
- [ ] **pull-XOR-push**: `pull_enabled` ⇒ push instructions (`seal`/`advance_*`/
      `begin_apply`) reject (`PullPushExclusive`); §9.4 has NO epoch gate (matches §10).
- [ ] **`reset` clears** `finalized`, `reduce_skipped_count`, `apply_skipped_count`;
      target pinned (`requires_reduce→Reducing`, else `Applying`; never `Pending`).
- [ ] **Skip counters split**: `reduce_skipped_count` (corrupts `acc_global` ⇒
      `assert_settled` requires `==0`) vs `apply_skipped_count` (benign). `Skipped` event
      carries a phase tag.
- [ ] **`init_shards` IDENTITY write is create-branch-only**; existing→fund-only, never
      touch acc_partial/cursors/epoch; forbidden once `status != Pending`.
- [ ] **Completion is cursor-derived** (v0.3): no `*_DONE`/`merged` flags; a phase is
      done iff `shard.epoch==effect.epoch ∧ phase_cursor==len`.
- [ ] **`reduce_shards` completeness**: ascending `shard_id==merge_cursor` (exactly-once
      merge), `epoch==e.epoch ∧ reduce_cursor==len`, `Reduced` only at `merge_cursor==P`;
      `begin_apply` asserts `Reduced`; **`try_finish_apply` asserts `Applying`**.
- [ ] **`reset`**: forbidden if `pull_enabled`; routes `requires_reduce` to
      `Pending`/`Reducing` (never `Applying`); **clears `finalized`**; resets
      `shards_done`/`merge_cursor`/`acc_global=IDENTITY`.
- [ ] **`seal` asserts `1 ≤ shard_count ≤ total`** (no empty shards); `shard_id <
      shard_count` asserted in `advance_*`; partition arithmetic in **u64**.
- [ ] **`IDENTITY` initialized explicitly** at `init_effect` (`acc_global`) and
      `init_shards`/first-touch (`acc_partial`) — zeroed bytes ≠ `IDENTITY` for non-zero
      monoids (MAX/min/top-k).
- [ ] **Two-phase funding `2×`** for `requires_reduce` (REDUCE + APPLY both pay).
- [ ] **`refund_escrow` sweeps to rent floor, NEVER closes**; gated `Done`/`Cancelled` ∧
      `last_refund_epoch < epoch`; `finalize` idempotent via `finalized`.
- [ ] **Pull (if enabled)**: no epoch gate; `saturating_sub`; `reset` forbidden.
- [ ] **`acc_external` asserted OFF** at `init_effect` (deferred, §2.5/§15.3).
- [ ] **Checked enum** `status`; **`BadVersion`** on `version > SUPPORTED`.
- [ ] **No arbitrary CPI**: token payouts go only to the declared token program;
      `apply` receives no escrow/signer authority (all signing is in `invoke_signed`).
- [ ] **Clock**: no consensus-critical `Clock` dependence; `created_slot` observability.

---

## 12. Error codes (stable; reused across frameworks)
```
0  WrongPhase        4  AlreadyApplied   8  BadBump            12 BountyUnderfunded
1  ShardComplete     5  IndexMismatch    9  SentinelExpected   13 NotShardable
2  NotSettled        6  BadDiscriminator 10 SlotOccupied       14 ApplyFailed(consumer)
3  EpochMismatch     7  BadOwner         11 Unauthorized       15 BadVersion
16 MergeOrder        17 PullNoReset      18 ShardIdOutOfRange  19 AccExternalDeferred
20 PullPushExclusive 21 EpochUninit       22 ShardsIncomplete   23 NotSealed
24 AccountDataTooSmall 25 BadStatus
```
Wire orphan codes to asserts: `MergeOrder(16)` in `reduce_shards` (`shard_id==merge_cursor`),
`ShardIdOutOfRange(18)` in `advance_*` (`shard_id<shard_count`), `AccExternalDeferred(19)`
in `init_effect`, `PullPushExclusive(20)` in push instructions, `EpochUninit(21)` in `seal`.

## 13. Events (single framework-agnostic encoding)
Use raw `sol_log_data` with a fixed schema `[tag:u8][effect:Pubkey][epoch:u64]
[payload…]` in ALL three builds (NOT Anchor `emit!`), so indexing is identical and one
reference indexer is published. Events: `Initialized · Enrolled{range} · Sealed{total,P}
· Advanced{shard,phase,to,n} · ShardMerged{shard} · Reduced · Done · Finalized ·
ToppedUp{shard} · Refunded{shard} · Cancelled · Reset{epoch} · Skipped{index}`.

## 14. CU model & benchmark methodology (honest framing)
freshet's **overhead per batch is deterministic** (cursor math + N derivations + header
checks + escrow binding). **Total CU = overhead + N × cost(C::apply)** — `cost(apply)`
is consumer-defined and opaque; a token-transfer-per-winner `apply` likely forces
`BATCH ≤ 2–4`, trivial arithmetic allows `BATCH = 16–32`. The benchmark MUST report
**freshet overhead separately from total**, across Quasar/Pinocchio/Anchor, measured
with `mollusk`/`litesvm`, harness published. Required property tests:
- **monoid laws** (assoc/commut/identity);
- **shard-count invariance** (same `acc_global` for P=1 vs P=k) — run with a
  **non-zero-identity monoid** (e.g. MAX over all-negative members) to catch the
  IDENTITY-init bug;
- **two-phase funding bound**: total bounty paid over a full REDUCE+APPLY run ≤ funded;
- **pull skipped-epoch convergence**: one pull after k accruals == sum of k per-step
  pulls;
- **skip non-wedge**: skipping the last member of a shard still reaches `Done`.

Never claim "deterministic total CU".

## 15. Non-goals / open items
- **Non-goal:** cross-shard order guarantees; non-monoid aggregates (force `P=1`);
  live `migrate` (reserved); `reset` of pull effects (rotate via fresh `Effect`).
- **Resolved (v0.3):**
  1. **Quasar variable accounts — CONFIRMED supported** (commit `a89a932`, 2026-05-31):
     `CtxWithRemaining` + `RemainingAccounts`. Typed `Remaining<T,N>` caps at **64**
     (stack cache); the **raw `get(index)` accessor is uncapped** (Solana-bound).
     Decision: `enroll_batch` uses typed `Remaining<Member, N≤64>`; `reduce_shards`/
     `try_finish_apply` use the **raw accessor** (we validate every shard by hand
     anyway) → all three frameworks reach the same Solana-bound `CAP`. *Optional
     visibility sidequest: PR Quasar a const-generic cache so `MAX_REMAINING_ACCOUNTS`
     isn't hard-coded 64.*
  2. **`CAP` is a per-framework compile-time const** (correctness is independent of it —
     `merge_cursor`/`shards_done` tolerate any chunking): default to the Solana-bound max
     `≈120` (ALT mandatory) on all three; sub-64 only if a framework forces it. Benchmark
     reports both same-CAP (apples-to-apples) and max-CAP throughput.
  3. **Pull skipped-epoch:** resolved — cumulative `acc` + `saturating_sub`, no epoch
     gate, no `reset` (§10). No per-epoch snapshots (rejected: one account/epoch cost).
  4. **`acc_external`:** OUT of the v0.3 reference impl — inline-only ≤64 B; `init_effect`
     asserts `!acc_external` (`AccExternalDeferred`). Future extension (§2.5).
- **Open / freeze prerequisites:**
  5. **Formal model-check (REQUIRED before freeze)** — model §4 exactly-once
     (intra+cross-epoch) and the §5 phase machine (no premature/unreachable
     `Done`/`Reduced`, cursor-derived completion, lazy-reset, the reset/refund/finalized
     lifecycle) in TLA+ or Kani. The v0.2 re-verification showed these liveness wedges
     slip past human review; a model-check would have caught the `skip` wedge and the
     `finalized`-flag lockout. **Run after the Pinocchio reference impl, before freeze.**
  6. Re-run the adversarial re-verification on v0.3 to confirm the v0.2→v0.3 fixes hold
     and introduced no new regressions.
```
