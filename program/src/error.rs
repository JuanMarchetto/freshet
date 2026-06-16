//! Stable `ProgramError::Custom` code table. Core state-machine errors map through
//! `map()`; the IO-layer codes below are used directly by the security helpers. Every
//! code is named for greppability and to keep call sites self-documenting; the mapping
//! is hand-written because the core enum's order differs from this numbering.

use freshet::state::Error as CoreErr;
use pinocchio::program_error::ProgramError;

// Core state-machine codes (produced via `map`).
pub const E_WRONG_PHASE: u32 = 0;
pub const E_SHARD_COMPLETE: u32 = 1;
pub const E_SHARD_NOT_COMPLETE: u32 = 2;
pub const E_ALREADY_FINALIZED: u32 = 4;
pub const E_NOT_SHARDABLE: u32 = 13; // also BadShardCount (only NotShardable is defined in §12)
pub const E_MERGE_ORDER: u32 = 16;
pub const E_PULL_NO_RESET: u32 = 17;
pub const E_SHARD_ID_OUT_OF_RANGE: u32 = 18;
pub const E_PULL_PUSH_EXCLUSIVE: u32 = 20;
pub const E_EPOCH_UNINIT: u32 = 21;
pub const E_SHARDS_INCOMPLETE: u32 = 22;
pub const E_NOT_SEALED: u32 = 23;
pub const E_BAD_STATUS: u32 = 25;

// IO-layer codes (used directly by the security helpers).
pub const E_EPOCH_MISMATCH: u32 = 3;
pub const E_INDEX_MISMATCH: u32 = 5;
pub const E_BAD_DISCRIMINATOR: u32 = 6;
pub const E_BAD_OWNER: u32 = 7;
pub const E_BAD_BUMP: u32 = 8;
pub const E_SENTINEL_EXPECTED: u32 = 9;
pub const E_SLOT_OCCUPIED: u32 = 10;
pub const E_UNAUTHORIZED: u32 = 11;
pub const E_BOUNTY_UNDERFUNDED: u32 = 12;
pub const E_APPLY_FAILED: u32 = 14;
pub const E_BAD_VERSION: u32 = 15;
pub const E_ACC_EXTERNAL_DEFERRED: u32 = 19;
pub const E_ACCOUNT_DATA_TOO_SMALL: u32 = 24;

/// Map a core state-machine error to its stable custom code.
pub fn map(e: CoreErr) -> ProgramError {
    let code = match e {
        CoreErr::WrongPhase => E_WRONG_PHASE,
        CoreErr::ShardComplete => E_SHARD_COMPLETE,
        CoreErr::ShardNotComplete => E_SHARD_NOT_COMPLETE,
        CoreErr::AlreadyFinalized => E_ALREADY_FINALIZED,
        CoreErr::BadShardCount | CoreErr::NotShardable => E_NOT_SHARDABLE,
        CoreErr::MergeOrder => E_MERGE_ORDER,
        CoreErr::PullNoReset => E_PULL_NO_RESET,
        CoreErr::ShardIdOutOfRange => E_SHARD_ID_OUT_OF_RANGE,
        CoreErr::PullPushExclusive => E_PULL_PUSH_EXCLUSIVE,
        CoreErr::EpochUninit => E_EPOCH_UNINIT,
        CoreErr::ShardsIncomplete => E_SHARDS_INCOMPLETE,
        CoreErr::NotSealed => E_NOT_SEALED,
        CoreErr::BadStatus => E_BAD_STATUS,
    };
    ProgramError::Custom(code)
}

pub fn custom(code: u32) -> ProgramError {
    ProgramError::Custom(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(e: CoreErr) -> u32 {
        match map(e) {
            ProgramError::Custom(c) => c,
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn core_err_maps_to_spec_12_codes() {
        assert_eq!(code(CoreErr::WrongPhase), 0);
        assert_eq!(code(CoreErr::ShardComplete), 1);
        assert_eq!(code(CoreErr::ShardNotComplete), 2);
        assert_eq!(code(CoreErr::AlreadyFinalized), 4);
        assert_eq!(code(CoreErr::BadShardCount), 13);
        assert_eq!(code(CoreErr::NotShardable), 13); // N:1 (only NotShardable=13 in §12)
        assert_eq!(code(CoreErr::MergeOrder), 16);
        assert_eq!(code(CoreErr::PullNoReset), 17);
        assert_eq!(code(CoreErr::ShardIdOutOfRange), 18);
        assert_eq!(code(CoreErr::PullPushExclusive), 20);
        assert_eq!(code(CoreErr::EpochUninit), 21);
        assert_eq!(code(CoreErr::ShardsIncomplete), 22);
        assert_eq!(code(CoreErr::NotSealed), 23);
        assert_eq!(code(CoreErr::BadStatus), 25);
    }
}
