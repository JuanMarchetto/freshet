//! freshet — a resumable, permissionless, shardable MapReduce over Solana accounts.
//!
//! See `SPEC.md` for the authoritative contract. This crate is the framework-agnostic
//! core; the pure logic (partition math, state machine, exactly-once invariants) is
//! unit-tested here without a Solana runtime. It is `no_std` (the on-chain `program`
//! crate links it); the host-only executable model (`state::Machine`) is behind `std`.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod monoid;
pub mod partition;
pub mod state;
