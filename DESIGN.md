# freshet — A Resumable Effect Engine for Solana

> **Renamed Cascade → `freshet`** (2026-06-16; "cascade"/"cascada" taken on crates.io).
> "Cascade" below = `freshet`. Authoritative spec: `SPEC.md` (v0.4). Crate `freshet`,
> org `freshet-rs`, demos `freshet-royale`.

> Working name: **Cascade** (alt: *Crankset*, *Sweep*, *Tick*). The on-chain account
> that holds the rule + progress is the `Effect` PDA — hence the project dir name.
>
> ⚠️ **`SPEC.md` is the authoritative contract** and supersedes the mechanism sketches
> here. After adversarial review (2026-06-15) the design hardened in four ways this doc
> only partially reflects: (1) Cascade is a **library/crate**, not a standalone program
> (account-ownership constraint); (2) an explicit **REDUCE→MAP two-phase** model so you
> can *evaluate* N accounts to compute a result before *applying* it; (3) **sharding**
> (per-partition cursors/escrows) because a single `Effect` writer defeats Sealevel
> parallelism; (4) exactly-once needs a **per-member `epoch` stamp** for re-runs. See
> `SPEC.md` §0–§9.

A reusable on-chain primitive that lets a single logical event modify an
**unbounded number of accounts**, by partitioning the work into permissionless,
resumable, idempotent crank batches — plus a complementary pull-based (lazy) mode
for effects expressible as a formula.

Built three times, on purpose: **Quasar → Pinocchio → Anchor**, with a rigorous
CU / LOC / DX benchmark across all three. The cross-framework comparison is the
headline artifact.

---

## 1. The problem (validated, with primary-source numbers)

A Solana transaction must declare **every** account it reads or writes, up front.
That list is bounded:

| Limit | Value (2026) | Source |
|---|---|---|
| Transaction size | **1232 bytes** (hard, IPv6 MTU) | Anza docs |
| Accounts addressable without ALT | ~**35** | Anza docs |
| Accounts addressable with ALT (`u8` index) | **256** | Anza docs |
| Accounts **lockable** per tx (`MAX_TX_ACCOUNT_LOCKS`) | **128** (raised from 64 in v1.14.17) | solana #27241 |
| Compute units per tx | **1.4M CU** | Anza docs |

So the practical ceiling for "accounts atomically modified by one event" is ~128,
and in practice far less once real per-account compute is counted. This blocks:

- **On-chain games** where a world event must update many entities at once.
- **Prediction markets** that must pay out all winners on resolution.
- Any "evaluate N accounts → modify M accounts as a result" workflow.

Even Solana's own protocol hit this: **SIMD-0118 (Partitioned Epoch Reward
Distribution)** exists precisely because rewarding 550K+ stake accounts didn't fit
in a block, so the core itself partitions the work across blocks with a progress
pointer. That is the exact pattern Cascade generalizes for application developers.

### What already exists (and why there's still a gap)

- **Tuk Tuk** (Helium, active): a permissionless *cron/crank* engine. It is
  **transport** — it can fire our `advance` txs without a trusted operator — but it
  is **not** a settlement primitive (no rule PDA, no progress cursor, no
  idempotency, no accumulator). Cascade complements it, not competes with it.
- **Clockwork**: dead (shutdown 2023).
- **Drift / OpenBook / Mango**: each reimplements fan-out **inline and bespoke**.
  No reusable on-chain crate exists. **That is the gap.**

### Design lineage (cite this — it earns credibility)

- **Drift** — pull-based settlement via intermediate per-market P&L pools;
  `settlePNL` is permissionless. → our **pull mode** + permissionless crank.
- **OpenBook/Serum** — `consume_events` processes a **bounded batch** (default 19)
  per tx; the cranker is **rewarded from fees**. → our **batch size** + **keeper
  bounty**. OpenBook v2's "hybrid crank" (settle inline, crank the overflow) → our
  inline-first strategy.
- **SIMD-0118** — partitioned progress at protocol level. → our cursor model.

---

## 2. Why off-chain doesn't count

The whole point of putting a game or market on-chain is **verifiability,
transparency, censorship-resistance**. If a trusted server decides "who won and who
gets paid," the value proposition collapses. So we rank approaches by *trust
required*:

| Approach | Off-chain trust | Transparency | In scope? |
|---|---|---|---|
| Pull-based / lazy on-chain | none | total | ✅ core |
| Resumable crank (Cascade) + Tuk Tuk | none (permissionless keeper) | total | ✅ core |
| Off-chain verifiable (ZK / fraud proof) | low | high | later |
| Off-chain trusted (server signs) | total | none | ❌ rejected |

Cascade is **zero-trust**: the rule lives in an account, anyone can advance the
crank, and every state transition is verifiable on-chain.

---

## 3. The primitive

### 3.1 State — the `Effect` PDA (rule + progress)

```
Effect {
  status:        u8,        // Pending | InProgress | Done | Cancelled
  epoch:         u64,       // anti-replay; bumped per re-run
  cursor:        u64,       // progress pointer (next index to process)
  total:         u64,       // number of members in the set
  rule_id:       u16,       // which transition the program applies
  params:        [u8; N],   // the concrete change (delta, winner, formula args)
  accumulator:   [u8; M],   // running aggregate for pull-mode / cross-account state
  bounty:        u64,       // escrow to pay keepers
  bounty_per:    u64,       // reward per processed item (or per batch)
  authority:     Pubkey,    // who can init/cancel (may be a program/PDA)
}
```

### 3.2 Membership — PDA-by-index (DECIDED)

Each member's state lives at a PDA derived **from its index**:

```
member_pda(effect, i) = find_program_address(
    [b"member", effect.key, i.to_le_bytes()], program_id)
```

- **Enrollment**: `enroll(effect, data)` assigns the next index
  (`i = effect.total`), creates `member_pda(effect, i)` holding `data` (including
  the member's wallet if relevant), and increments `total`. Enrollment only allowed
  while `status == Pending`.
- **Zero-trust verification**: during `advance`, for each index `i` in the batch the
  caller passes an account; the program **re-derives** `member_pda(effect, i)` and
  asserts it equals the passed key. The caller cannot smuggle in foreign accounts.
- **Exactly-once within an epoch**: `cursor` is **monotonic** and the batch must cover
  `[cursor, cursor+batch)` **in order**, with the range **derived from the on-chain
  cursor** (never caller-supplied). An index can't be processed twice or skipped within
  one epoch. *Across* epochs (re-runs via `reset`) this is NOT enough — each member also
  stamps `last_apply_epoch` (see `SPEC.md` §4). The "no per-account flag" shortcut holds
  only for single-epoch effects.

> Trade-off accepted: members must be enrolled into sequential index slots. For sets
> that are naturally PDA-derived (game entities, market positions created by the
> program) this is free. For arbitrary external wallets, enrollment is one extra tx
> per member at setup time — acceptable, and still fully on-chain.

### 3.3 Instructions

1. `init_effect(rule_id, params, bounty, bounty_per)` → creates `Effect` in
   `Pending`. Funds the bounty escrow.
2. `enroll(effect, data)` → assigns index, creates `member_pda`, `total += 1`.
   (`Pending` only.)
3. `seal(effect)` → `Pending → InProgress`. Freezes `total`; no more enrollment.
4. `advance(effect, [member accounts for indices cursor..cursor+BATCH])` →
   **permissionless crank**:
   - assert `status == InProgress` and accounts match `member_pda(effect, i)` for
     `i in [cursor, cursor+BATCH)`, in order;
   - apply `rule_id`/`params` to each member (push mode), updating `accumulator` as
     needed;
   - `cursor += filled`; pay the cranker `filled * bounty_per` from the escrow;
   - if `cursor >= total` → `status = Done`.

   **DECIDED: fixed-width batch (`BATCH`, e.g. 8), NOT variable `remaining_accounts`.**
   The `advance` Accounts struct declares exactly `BATCH` member slots. Rationale:
   (a) Quasar's support for variable-length account processing is **unconfirmed**
   (all its examples use fixed account structs), and fixed-width sidesteps it
   entirely; (b) **deterministic Cascade *overhead* per crank** (total CU = overhead +
   BATCH × cost(apply), consumer-defined — do NOT claim deterministic *total* CU; see
   `SPEC.md` §14); (c) makes the Quasar / Pinocchio / Anchor benchmark **apples-to-
   apples** (identical batch width). The final batch may be partially filled: the fill
   count is **derived on-chain** as `min(BATCH, total − cursor)` (never caller-supplied),
   and tail slots use a checked sentinel (see `SPEC.md` §6.1).
   Pinocchio and Anchor *could* additionally support variable `remaining_accounts`,
   but we standardize on fixed-width across all three for comparability.
5. `finalize(effect)` → post-`Done` cleanup; emit event; unlock dependent reads.
6. `cancel(effect)` / `reset(effect)` → bump `epoch`, refund/abort (authority only).

### 3.4 Two modes

- **Push (crank)** — the engine writes to each member. **Required** when the effect
  must happen whether or not the member's owner shows up: game eliminations, world
  ticks, forced settlement. This is Cascade's unique territory; pull-based cannot do
  it.
- **Pull (lazy)** — the engine only commits `rule`/`accumulator`; each member
  computes its own update the next time it is touched, as
  `delta = f(accumulator, member.snapshot)`. No crank, O(1) event. Use whenever the
  effect *is* a formula and the owner is incentivized to come claim. (Drift's model.)

A single `Effect` can mix modes: settle inline what fits in the event tx, crank the
overflow, expose pull for late claimers (OpenBook v2 hybrid pattern).

### 3.5 Atomicity invariant (the sharp edge — design for it)

Cascade is **NOT atomic** across the full set. Between `seal` and `Done` the set is
half-updated. The contract:

- "Half-applied" is a **first-class valid state** (`InProgress`).
- Any logic that depends on the *completed* effect MUST gate on `status == Done`.
  Provide `assert_settled(effect)` as a guard helper.
- `epoch` prevents a re-run from double-applying to a member.

---

## 4. Keeper incentive & liveness

- **Bounty escrow** funded at `init_effect`; each `advance` pays the cranker
  `k * bounty_per` (OpenBook pattern). No trusted operator.
- **Optional Tuk Tuk integration**: register the `advance` loop as a Tuk Tuk task so
  it runs permissionlessly until `Done`, with the bounty covering Tuk Tuk's fee.
- **Liveness note**: if the bounty runs dry, the effect stalls in `InProgress` —
  acceptable and visible on-chain. Document recommended bounty sizing
  (`total / batch * batch_cost + margin`).

---

## 5. The three-framework rollout (the prestige artifact)

| Phase | Framework | Goal | Risk to manage |
|---|---|---|---|
| 1 | **Quasar** (blueshift-gg) | early-adopter visibility (Blueshift/Dean Little network); zero-copy story | beta, unaudited — demo/benchmark only, no real value; **file bugs = contributor cred** |
| 2 | **Pinocchio** (Anza) | canonical CU credibility; the "gold standard" number | low DX; ship Codama/Shank TS bindings separately |
| 3 | **Anchor** | legible reference impl; max integrability | higher CU overhead — it's the baseline in the benchmark |

**Headline deliverable**: a writeup/thread —
*"The fan-out problem on Solana, and a resumable-effect primitive built in Quasar,
Pinocchio & Anchor — benchmarked."* With a table:

| Framework | CU / `advance` (batch=16) | CU / `enroll` | Program bytes | Handler LOC | DX notes |
|---|---|---|---|---|---|
| Quasar | … | … | … | … | … |
| Pinocchio | … | … | … | … | … |
| Anchor | … | … | … | … | … |

**Benchmark rigor is the reputation.** The community will scrutinize CU claims;
measure with `mollusk`/`litesvm`, publish the harness, no hand-waving. (Note: a
widely-cited "Pinocchio = 90% less CU" figure failed verification — do not repeat
unverified numbers; produce your own.)

---

## 6. Open risks / caveats

- **Demand is unverified.** Research found a real *technical* gap but no explicit
  developer demand signal. Mitigate by leading with a viral demo (see `GAMES.md`),
  not by pitching "infrastructure" (the Clockwork failure mode: respected, unadopted,
  "limited commercial upside").
- **Quasar maturity**: beta, unaudited, no published benchmarks. Fine as
  phase-1 visibility vehicle; do not anchor production claims on it.
- **PDA-by-index enrollment cost** for arbitrary-wallet sets (one tx/member).
- **Not atomic** — must be communicated loudly; misuse = inconsistent reads.

---

## 7. Quasar feasibility (phase 1) — verdict: **GO, build against a pinned commit**

Verified against the Quasar repo (`blueshift-gg/quasar`, master) and
`quasar-lang.com/docs`, mid-2026.

**Confirmed (3-0) — everything the primitive's building blocks need:**

| Capability | Status | Note |
|---|---|---|
| PDA derivation (seeds + stored bump) | ✅ confirmed | `address = Type::seeds(...)`, stored `bump: u8`, ~2000 CU saved on re-verify. **PDA-by-index = trivial extension** of the demonstrated PDA-by-key. |
| `#[derive(Accounts)]` constraints | ✅ confirmed | `init`, `init_if_needed`, `mut`, `seeds/bump`, `has_one`, `constraint`, `address`, `close`, `realloc`, `dup`, `@Error` |
| PDA-signed CPI for payouts | ✅ confirmed | `transfer().invoke_signed(&seeds)` via `quasar-spl`; Token + Token-2022 |
| Zero-copy account data | ✅ confirmed | pointer-cast from SVM buffer, `no_std`, alignment-1 Pod |
| Test harness | ✅ confirmed | **QuasarSVM** (in-process, Rust/Node/Python), supports `find_program_address` |

**The one gap that shaped the design (unresolved):** variable-length account
processing (`remaining_accounts` / slices / handler loop over accounts) is **NOT
confirmed** — all Quasar examples use fixed account structs. → Resolved by the
**fixed-width `BATCH` decision** in §3.3. (Optional: confirm by reading the
`quasar-derive` macro source or asking maintainers — and if it's genuinely missing,
**that's a contribution opportunity = visibility**.)

**Maturity caveats (build accordingly):**
- Beta, **unaudited, no published releases** ("APIs may change, use at your own
  risk"). → **Pin to a specific git commit**, never "latest". Demo/benchmark only —
  no real value on a Quasar deployment.
- Syntax differs from Anchor (`has_one(creator)`, `address = Type::seeds(...)`) and
  can drift without a release boundary.
- **No published CU benchmarks** for the "near-hand-written efficiency" claim. → This
  is *our* opening: produce the first rigorous Quasar-vs-Pinocchio-vs-Anchor
  benchmark. That artifact is the prestige play.

**Visibility verdict (medium confidence):** Blueshift / Dean Little are real, active
(live courses, Helius-profiled), and Quasar/QuasarSVM are early (`quasar-svm` ~23★) —
a genuine early-adopter window. But **no bounty/grant/early-adopter program was
confirmed**. Convert visibility via *contribution* (file issues, fix the
variable-accounts gap, publish the benchmark), not by assuming a program exists.

**Open items to close before/at build start:**
1. Confirm or rule out variable-account support in `quasar-derive` (else fixed-width
   stands — which is fine).
2. Check `events`/logging support for `finalize` emit.
3. Pin the Quasar commit hash in `Cargo.toml`.

## 8. Status

- [x] Membership model decided: **PDA-by-index**.
- [x] Quasar feasibility research → **GO** (fixed-width batch; pin commit).
- [x] Batch model decided: **fixed-width `BATCH` (e.g. 8)**.
- [ ] Formal spec freeze.
- [ ] Phase 1: Quasar implementation + demo.
- [ ] Phase 2: Pinocchio.
- [ ] Phase 3: Anchor.
- [ ] Benchmark harness + writeup.
