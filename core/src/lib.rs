//! **freshet** — a resumable, permissionless, shardable *MapReduce over Solana accounts*.
//!
//! A Solana transaction can only lock ~128 accounts and spend 1.4M CU, so a single event
//! cannot atomically modify an unbounded set of accounts. freshet is the on-chain pattern
//! that lifts that ceiling: one logical effect is partitioned into **permissionless,
//! resumable, idempotent crank batches**, optionally in two phases —
//!
//! - **REDUCE** — scan N member accounts into a commutative-monoid accumulator
//!   (e.g. tally a histogram, find a winner), then
//! - **MAP** — apply the result to every member, *including members whose owner never
//!   shows up* (push-mode), which a pull-based "claim it yourself" design cannot express.
//!
//! Exactly-once is guaranteed by a monotonic cursor within an epoch plus a per-member
//! epoch stamp across re-runs; liveness by a keeper bounty. See the [`SPEC`] for the full
//! contract.
//!
//! # What this crate is
//!
//! This is the **framework-agnostic core** — pure, `no_std`, alloc-free logic with no
//! Solana dependency:
//!
//! - [`partition`] — the deterministic member→shard partition (§2.8), computed never stored.
//! - [`state`] — the verified state-machine guards ([`advance_step`](state::advance_step)
//!   et al.) that drive every phase transition, plus a host-only executable model
//!   ([`Machine`](state::Machine), behind the `std` feature) that the property tests
//!   model-check.
//! - [`monoid`] — the [`Monoid`](monoid::Monoid) trait for the REDUCE accumulator, with
//!   [`Sum`](monoid::Sum) and [`MaxWinner`](monoid::MaxWinner).
//!
//! freshet is a **library linked into the consumer program**, not a standalone program —
//! only the program that owns an account may write it. The consumer owns its account
//! layout and delegates control-flow to these guards, so its handlers run the verified
//! logic rather than a re-implementation. Reference settlers in Pinocchio, Anchor, and
//! Quasar (with a cross-framework CU benchmark) live in the [repository].
//!
//! # Example
//!
//! ```
//! use freshet::partition::{partition_len, partition_start};
//! use freshet::monoid::{Monoid, Sum};
//!
//! // 10 members over 3 shards tile [0,10) as 4 + 3 + 3, contiguous and gap-free.
//! assert_eq!(partition_len(10, 3, 0), 4);
//! assert_eq!(partition_len(10, 3, 1), 3);
//! assert_eq!(partition_start(10, 3, 1), 4);
//!
//! // A REDUCE accumulator merges associatively, so shards combine in any order/count.
//! assert_eq!(Sum(3).combine(Sum(4)).combine(Sum(0)), Sum(7));
//! ```
//!
//! # Features
//!
//! - `std` *(default)* — enables the host-only [`Machine`](state::Machine) model used by
//!   the property tests. Disable it (`default-features = false`) for the `no_std`,
//!   alloc-free surface that on-chain programs link.
//!
//! [`SPEC`]: https://github.com/JuanMarchetto/freshet/blob/main/SPEC.md
//! [repository]: https://github.com/JuanMarchetto/freshet
#![cfg_attr(not(feature = "std"), no_std)]

pub mod monoid;
pub mod partition;
pub mod state;
