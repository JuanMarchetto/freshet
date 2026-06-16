//! Executable model of the freshet state machine: the pure transition guards plus a
//! host-only `Machine` that exercises them. The on-chain Pinocchio handlers delegate
//! their control flow to the same guards, so both run identical logic. The property
//! tests cover every guard, cursor-derived completion, the reset clear-set, lazy-reset,
//! and the safety/liveness invariants. See `SPEC.md` for the contract.

#[cfg(feature = "std")]
use crate::partition::partition_len;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Status {
    Pending = 0,
    Reducing = 1,
    Reduced = 2,
    Applying = 3,
    Done = 4,
    Cancelled = 5,
}

impl core::convert::TryFrom<u8> for Status {
    type Error = Error;
    /// Checked decode of the on-chain status byte (§2.1: "checked conversion, never raw
    /// match"). Out-of-range (≥6) is corruption, not a legitimate phase.
    fn try_from(v: u8) -> Result<Status, Error> {
        match v {
            0 => Ok(Status::Pending),
            1 => Ok(Status::Reducing),
            2 => Ok(Status::Reduced),
            3 => Ok(Status::Applying),
            4 => Ok(Status::Done),
            5 => Ok(Status::Cancelled),
            _ => Err(Error::BadStatus),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    WrongPhase,
    NotShardable,
    PullPushExclusive,
    PullNoReset,
    EpochUninit,
    ShardsIncomplete,
    NotSealed,
    BadShardCount,
    ShardComplete,
    ShardIdOutOfRange,
    MergeOrder,
    ShardNotComplete,
    AlreadyFinalized,
    BadStatus,
}

// ── Pure transition guards (no_std, no alloc) ──────────────────────────────────────
// The single source of truth for the engine's control flow. BOTH the host `Machine`
// (model-checked by the property tests) and the on-chain Pinocchio handlers call these,
// so the handler executes the verified logic rather than a re-implementation.

/// `advance_reduce`/`advance_apply` guard: given the POST-lazy-reset `cursor`, the shard
/// `len`, and `batch`, returns how many members to process. The caller checks phase +
/// shard_id and runs the lazy-reset prologue FIRST (so the reset predicate lives in one
/// place, not here).
pub fn advance_step(cursor: u64, len: u64, batch: u64) -> Result<u64, Error> {
    if cursor >= len {
        return Err(Error::ShardComplete);
    }
    Ok(batch.min(len - cursor))
}

/// `skip`/`skip_reduce` guard: the post-reset cursor must have room for one more.
pub fn skip_step(cursor: u64, len: u64) -> Result<(), Error> {
    if cursor >= len {
        return Err(Error::ShardComplete);
    }
    Ok(())
}

/// `reduce_shards` per-shard guard: may this shard be merged now? Ascending order +
/// fully-reduced + current-epoch. Caller does the single `merge_cursor += 1` and the
/// `combine` STRICTLY after this returns Ok.
pub fn reduce_step(
    merge_cursor: u32,
    shard_id: u32,
    shard_epoch: u64,
    effect_epoch: u64,
    reduce_cursor: u64,
    len: u64,
) -> Result<(), Error> {
    if shard_id != merge_cursor {
        return Err(Error::MergeOrder);
    }
    if shard_epoch != effect_epoch || reduce_cursor != len {
        return Err(Error::ShardNotComplete);
    }
    Ok(())
}

/// `try_finish_apply` per-shard guard: ascending order + fully-applied + current-epoch.
pub fn apply_finish_step(
    shards_done: u32,
    shard_id: u32,
    shard_epoch: u64,
    effect_epoch: u64,
    apply_cursor: u64,
    len: u64,
) -> Result<(), Error> {
    if shard_id != shards_done {
        return Err(Error::MergeOrder);
    }
    if shard_epoch != effect_epoch || apply_cursor != len {
        return Err(Error::ShardNotComplete);
    }
    Ok(())
}

/// Per-shard control state (cursors + the epoch for lazy reset). `len` is computed.
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug)]
pub struct ShardState {
    pub epoch: u64,
    pub reduce_cursor: u64,
    pub apply_cursor: u64,
}

/// Pure model of the `Effect` control fields that drive transitions. Host-only (uses
/// `Vec`); the on-chain handlers hold this state in account bytes, not a `Machine`.
#[cfg(feature = "std")]
pub struct Machine {
    pub status: Status,
    pub epoch: u64,
    pub total: u64,
    pub shard_count: u32,
    pub shards_created: u32,
    pub shards_done: u32,
    pub merge_cursor: u32,
    pub reduce_skipped: u64,
    pub apply_skipped: u64,
    pub finalized: bool,
    pub requires_reduce: bool,
    pub pull_enabled: bool,
    pub ever_sealed: bool,
    pub shards: Vec<ShardState>,
}

#[cfg(feature = "std")]
impl Machine {
    /// `init_effect`: create in Pending with epoch = 1 (0 is the never-applied
    /// sentinel). Asserts shardability.
    pub fn init(
        shard_count: u32,
        requires_reduce: bool,
        order_independent: bool,
        pull_enabled: bool,
    ) -> Result<Machine, Error> {
        if shard_count < 1 {
            return Err(Error::BadShardCount);
        }
        if shard_count > 1 && !order_independent {
            return Err(Error::NotShardable);
        }
        Ok(Machine {
            status: Status::Pending,
            epoch: 1, // 0 reserved as the never-applied sentinel
            total: 0,
            shard_count,
            shards_created: 0,
            shards_done: 0,
            merge_cursor: 0,
            reduce_skipped: 0,
            apply_skipped: 0,
            finalized: false,
            requires_reduce,
            pull_enabled,
            ever_sealed: false,
            shards: Vec::new(),
        })
    }

    /// `enroll`/`enroll_batch`: Pending only; bumps `total`.
    pub fn enroll(&mut self, k: u64) -> Result<(), Error> {
        if self.status != Status::Pending {
            return Err(Error::WrongPhase);
        }
        self.total += k;
        Ok(())
    }

    /// `init_shards` create branch: Pending only; creates `n` shard PDAs and bumps `shards_created`
    /// (the counter `seal` checks so every declared shard exists). Each shard starts at epoch 0 (lazy-reset on first
    /// touch); `acc_partial = IDENTITY` is modeled elsewhere (monoid module).
    pub fn create_shards(&mut self, n: u32) -> Result<(), Error> {
        if self.status != Status::Pending {
            return Err(Error::WrongPhase);
        }
        for _ in 0..n {
            self.shards.push(ShardState {
                epoch: 0,
                reduce_cursor: 0,
                apply_cursor: 0,
            });
            self.shards_created += 1;
        }
        Ok(())
    }

    fn partition_valid(&self) -> bool {
        let p = self.shard_count as u64;
        p >= 1 && p <= self.total
    }

    /// `seal`: freeze and enter the first work phase.
    pub fn seal(&mut self) -> Result<(), Error> {
        if self.status != Status::Pending {
            return Err(Error::WrongPhase);
        }
        if self.pull_enabled {
            return Err(Error::PullPushExclusive);
        }
        if self.epoch < 1 {
            return Err(Error::EpochUninit);
        }
        if !self.partition_valid() {
            return Err(Error::BadShardCount); // 1 <= shard_count <= total (no empty shards)
        }
        if self.shards_created != self.shard_count {
            return Err(Error::ShardsIncomplete);
        }
        self.ever_sealed = true;
        self.shards_done = 0;
        self.status = if self.requires_reduce {
            Status::Reducing
        } else {
            Status::Applying
        };
        Ok(())
    }

    /// `cancel`: any non-terminal state → Cancelled.
    pub fn cancel(&mut self) -> Result<(), Error> {
        if self.status == Status::Done || self.status == Status::Cancelled {
            return Err(Error::WrongPhase);
        }
        self.status = Status::Cancelled;
        Ok(())
    }

    /// `reset`: re-run from a terminal state.
    pub fn reset(&mut self) -> Result<(), Error> {
        if self.status != Status::Done && self.status != Status::Cancelled {
            return Err(Error::WrongPhase);
        }
        if self.pull_enabled {
            return Err(Error::PullNoReset);
        }
        if !self.ever_sealed {
            return Err(Error::NotSealed); // a cancel before the first seal must not be resettable
        }
        if !self.partition_valid() {
            return Err(Error::BadShardCount);
        }
        self.epoch += 1;
        self.shards_done = 0;
        self.merge_cursor = 0;
        self.reduce_skipped = 0;
        self.apply_skipped = 0;
        self.finalized = false;
        // acc_global = IDENTITY (monoid module); shards lazy-reset on next touch.
        self.status = if self.requires_reduce {
            Status::Reducing
        } else {
            Status::Applying
        };
        Ok(())
    }

    /// `len(shard_id)` — §2.8 computed partition length.
    fn len(&self, shard_id: u32) -> u64 {
        partition_len(self.total, self.shard_count as u64, shard_id as u64)
    }

    /// Universal lazy per-epoch reset prologue (§5) — run by EVERY shard-mutating op
    /// (advance_*, skip, skip_reduce) before touching the cursor.
    fn lazy_reset(&mut self, shard_id: u32) {
        let e = self.epoch;
        let s = &mut self.shards[shard_id as usize];
        if s.epoch < e {
            s.reduce_cursor = 0;
            s.apply_cursor = 0;
            // acc_partial = IDENTITY (monoid module)
            s.epoch = e;
        }
    }

    /// `advance_reduce`: fold up to `batch` members of `shard_id` into acc_partial.
    /// Returns the count processed. Effect-read-only on-chain; here it advances the
    /// shard's reduce_cursor. Pay/escrow is the economics layer, not modeled here.
    pub fn advance_reduce(&mut self, shard_id: u32, batch: u64) -> Result<u64, Error> {
        if self.status != Status::Reducing {
            return Err(Error::WrongPhase);
        }
        if shard_id >= self.shard_count {
            return Err(Error::ShardIdOutOfRange);
        }
        self.lazy_reset(shard_id); // universal prologue FIRST
        let len = self.len(shard_id);
        let cur = self.shards[shard_id as usize].reduce_cursor;
        let n = advance_step(cur, len, batch)?;
        self.shards[shard_id as usize].reduce_cursor += n;
        Ok(n)
    }

    /// `skip_reduce`: authority poison escape; advance reduce_cursor by 1.
    pub fn skip_reduce(&mut self, shard_id: u32) -> Result<(), Error> {
        if self.status != Status::Reducing {
            return Err(Error::WrongPhase);
        }
        if shard_id >= self.shard_count {
            return Err(Error::ShardIdOutOfRange);
        }
        self.lazy_reset(shard_id);
        let len = self.len(shard_id);
        skip_step(self.shards[shard_id as usize].reduce_cursor, len)?;
        self.shards[shard_id as usize].reduce_cursor += 1;
        self.reduce_skipped += 1; // CORRUPTS acc_global — surfaced by assert_settled
        Ok(())
    }

    /// `reduce_shards`: incrementally merge done shards in ascending order. No
    /// lazy-reset (read-only promotion op); a stale shard fails the epoch assert.
    pub fn reduce_shards(&mut self, shard_ids: &[u32]) -> Result<(), Error> {
        if self.status != Status::Reducing {
            return Err(Error::WrongPhase);
        }
        for &sid in shard_ids {
            let len = self.len(sid);
            let s = self.shards[sid as usize];
            reduce_step(
                self.merge_cursor,
                sid,
                s.epoch,
                self.epoch,
                s.reduce_cursor,
                len,
            )?;
            // acc_global = combine(acc_global, acc_partial) — monoid module (handler)
            self.merge_cursor += 1;
        }
        if self.merge_cursor == self.shard_count {
            self.status = Status::Reduced;
        }
        Ok(())
    }

    /// `begin_apply`: Reduced → Applying; reset shards_done.
    pub fn begin_apply(&mut self) -> Result<(), Error> {
        if self.status != Status::Reduced {
            return Err(Error::WrongPhase);
        }
        if self.pull_enabled {
            return Err(Error::PullPushExclusive);
        }
        self.shards_done = 0;
        self.status = Status::Applying;
        Ok(())
    }

    /// `advance_apply`: apply up to `batch` members of `shard_id`.
    pub fn advance_apply(&mut self, shard_id: u32, batch: u64) -> Result<u64, Error> {
        if self.status != Status::Applying {
            return Err(Error::WrongPhase);
        }
        if shard_id >= self.shard_count {
            return Err(Error::ShardIdOutOfRange);
        }
        self.lazy_reset(shard_id);
        let len = self.len(shard_id);
        let cur = self.shards[shard_id as usize].apply_cursor;
        let n = advance_step(cur, len, batch)?;
        self.shards[shard_id as usize].apply_cursor += n;
        Ok(n)
    }

    /// `skip`: authority poison escape; advance apply_cursor by 1. Runs the lazy
    /// prologue FIRST (else a skip as the first touch of a stale shard wedges the re-run).
    pub fn skip(&mut self, shard_id: u32) -> Result<(), Error> {
        if self.status != Status::Applying {
            return Err(Error::WrongPhase);
        }
        if shard_id >= self.shard_count {
            return Err(Error::ShardIdOutOfRange);
        }
        self.lazy_reset(shard_id);
        let len = self.len(shard_id);
        skip_step(self.shards[shard_id as usize].apply_cursor, len)?;
        self.shards[shard_id as usize].apply_cursor += 1;
        self.apply_skipped += 1; // benign per-member
        Ok(())
    }

    /// `try_finish_apply`: incrementally count done shards in ascending order; Done at
    /// shards_done == shard_count. Completion is cursor-derived (no per-shard done flag),
    /// so a `skip`-advanced cursor counts toward Done identically to a processed one —
    /// skipping the last member can never stall the phase.
    pub fn try_finish_apply(&mut self, shard_ids: &[u32]) -> Result<(), Error> {
        if self.status != Status::Applying {
            return Err(Error::WrongPhase);
        }
        for &sid in shard_ids {
            let len = self.len(sid);
            let s = self.shards[sid as usize];
            apply_finish_step(
                self.shards_done,
                sid,
                s.epoch,
                self.epoch,
                s.apply_cursor,
                len,
            )?;
            self.shards_done += 1;
        }
        if self.shards_done == self.shard_count {
            self.status = Status::Done;
        }
        Ok(())
    }

    /// `finalize`: Done ∧ !finalized → set finalized.
    pub fn finalize(&mut self) -> Result<(), Error> {
        if self.status != Status::Done {
            return Err(Error::WrongPhase);
        }
        if self.finalized {
            return Err(Error::AlreadyFinalized);
        }
        self.finalized = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_u8_is_stable_and_checked() {
        use core::convert::TryFrom;
        let all = [
            (0u8, Status::Pending),
            (1, Status::Reducing),
            (2, Status::Reduced),
            (3, Status::Applying),
            (4, Status::Done),
            (5, Status::Cancelled),
        ];
        for (v, s) in all {
            assert_eq!(s as u8, v, "discriminant pinned");
            assert_eq!(Status::try_from(v), Ok(s));
        }
        for v in 6u8..=255 {
            assert!(
                Status::try_from(v).is_err(),
                "byte {v} must reject (corruption)"
            );
        }
    }

    #[test]
    fn init_starts_pending_at_epoch_1() {
        let m = Machine::init(1, false, false, false).unwrap();
        assert_eq!(m.status, Status::Pending);
        assert_eq!(
            m.epoch, 1,
            "epoch 0 is the never-applied sentinel; init must use 1"
        );
        assert_eq!(m.total, 0);
        assert!(!m.ever_sealed);
    }

    /// Helper: a sealed single-pass effect with `total` members across `p` shards.
    fn sealed_single_pass(total: u64, p: u32) -> Machine {
        let mut m = Machine::init(p, false, true, false).unwrap();
        m.enroll(total).unwrap();
        m.create_shards(p).unwrap();
        m.seal().unwrap();
        m
    }

    #[test]
    fn seal_single_pass_enters_applying() {
        let m = sealed_single_pass(5, 1);
        assert_eq!(m.status, Status::Applying);
        assert!(m.ever_sealed);
        assert_eq!(m.total, 5);
    }

    #[test]
    fn seal_two_phase_enters_reducing() {
        let mut m = Machine::init(1, true, false, false).unwrap();
        m.enroll(5).unwrap();
        m.create_shards(1).unwrap();
        m.seal().unwrap();
        assert_eq!(m.status, Status::Reducing);
    }

    #[test]
    fn seal_rejects_incomplete_shards() {
        // shard_count 2 but only 1 shard created
        let mut m = Machine::init(2, false, true, false).unwrap();
        m.enroll(4).unwrap();
        m.create_shards(1).unwrap();
        assert_eq!(m.seal(), Err(Error::ShardsIncomplete));
    }

    #[test]
    fn seal_rejects_more_shards_than_members() {
        let mut m = Machine::init(3, false, true, false).unwrap();
        m.enroll(2).unwrap(); // total=2 < shard_count=3 → empty shard would result
        m.create_shards(3).unwrap();
        assert_eq!(m.seal(), Err(Error::BadShardCount));
    }

    #[test]
    fn seal_rejects_pull_enabled() {
        let mut m = Machine::init(1, false, false, true).unwrap();
        m.enroll(3).unwrap();
        m.create_shards(1).unwrap();
        assert_eq!(m.seal(), Err(Error::PullPushExclusive));
    }

    #[test]
    fn reset_rejects_unsealed_effect() {
        // cancel from Pending (never sealed) then reset must be rejected
        let mut m = Machine::init(1, false, false, false).unwrap();
        m.cancel().unwrap();
        assert_eq!(m.status, Status::Cancelled);
        assert_eq!(m.reset(), Err(Error::NotSealed));
    }

    #[test]
    fn reset_after_sealed_cancel_bumps_epoch_and_clears() {
        let mut m = sealed_single_pass(5, 1);
        m.apply_skipped = 2; // simulate prior-run residue
        m.cancel().unwrap();
        m.reset().unwrap();
        assert_eq!(m.epoch, 2, "reset bumps epoch");
        assert_eq!(m.status, Status::Applying, "single-pass reset → Applying");
        assert_eq!(m.apply_skipped, 0, "reset clears skip counters");
        assert_eq!(m.reduce_skipped, 0);
        assert!(!m.finalized);
        assert_eq!(m.total, 5, "total unchanged across reset");
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn sealed(total: u64, p: u32, requires_reduce: bool) -> Machine {
        let mut m = Machine::init(p, requires_reduce, true, false).unwrap();
        m.enroll(total).unwrap();
        m.create_shards(p).unwrap();
        m.seal().unwrap();
        m
    }

    fn apply_shard_fully(m: &mut Machine, sid: u32, batch: u64) {
        while m.shards[sid as usize].apply_cursor < m.len(sid) {
            m.advance_apply(sid, batch).unwrap();
        }
    }
    fn reduce_shard_fully(m: &mut Machine, sid: u32, batch: u64) {
        while m.shards[sid as usize].reduce_cursor < m.len(sid) {
            m.advance_reduce(sid, batch).unwrap();
        }
    }
    fn ids(p: u32) -> Vec<u32> {
        (0..p).collect()
    }

    #[test]
    fn single_pass_cranks_to_done_with_full_coverage() {
        let mut m = sealed(10, 3, false);
        for sid in 0..3 {
            apply_shard_fully(&mut m, sid, 4);
        }
        m.try_finish_apply(&ids(3)).unwrap();
        assert_eq!(m.status, Status::Done);
        let covered: u64 = m.shards.iter().map(|s| s.apply_cursor).sum();
        assert_eq!(
            covered, 10,
            "every member applied exactly once (cursor coverage)"
        );
    }

    #[test]
    fn two_phase_cranks_to_done() {
        let mut m = sealed(8, 2, true);
        assert_eq!(m.status, Status::Reducing);
        for sid in 0..2 {
            reduce_shard_fully(&mut m, sid, 3);
        }
        m.reduce_shards(&ids(2)).unwrap();
        assert_eq!(m.status, Status::Reduced);
        m.begin_apply().unwrap();
        assert_eq!(m.status, Status::Applying);
        for sid in 0..2 {
            apply_shard_fully(&mut m, sid, 3);
        }
        m.try_finish_apply(&ids(2)).unwrap();
        assert_eq!(m.status, Status::Done);
    }

    #[test]
    fn skip_last_member_still_reaches_done() {
        // skipping the last member must not stall the phase
        let mut m = sealed(3, 1, false); // one shard, len 3
        m.advance_apply(0, 2).unwrap(); // cursor 2
        m.skip(0).unwrap(); // poison last member; cursor 3 == len
        assert_eq!(m.shards[0].apply_cursor, 3);
        m.try_finish_apply(&[0]).unwrap();
        assert_eq!(
            m.status,
            Status::Done,
            "skip of last member must still reach Done"
        );
        assert_eq!(m.apply_skipped, 1);
    }

    #[test]
    fn skip_as_first_touch_after_reset_does_not_wedge() {
        // after reset, a stale shard whose first op is skip must run the
        // lazy-reset prologue (cursor len->0), not advance a stale cursor.
        let mut m = sealed(3, 1, false);
        apply_shard_fully(&mut m, 0, 3);
        m.try_finish_apply(&[0]).unwrap();
        assert_eq!(m.status, Status::Done);
        m.reset().unwrap(); // epoch 2, status Applying; shard still stale (epoch 1, cursor 3)
        assert_eq!(m.epoch, 2);
        m.skip(0).unwrap(); // first touch: prologue resets cursor to 0, then skip -> 1
        assert_eq!(m.shards[0].epoch, 2, "prologue must bump shard epoch");
        assert_eq!(
            m.shards[0].apply_cursor, 1,
            "prologue reset cursor to 0, skip -> 1"
        );
        apply_shard_fully(&mut m, 0, 3); // process remaining 2
        m.try_finish_apply(&[0]).unwrap();
        assert_eq!(m.status, Status::Done, "re-run reaches Done, no wedge");
    }

    #[test]
    fn try_finish_apply_rejects_incomplete_shard() {
        let mut m = sealed(5, 1, false);
        m.advance_apply(0, 2).unwrap(); // cursor 2 < len 5
        assert_eq!(
            m.try_finish_apply(&[0]),
            Result::Err(Error::ShardNotComplete)
        );
        assert_eq!(m.status, Status::Applying, "must not prematurely Done");
    }

    #[test]
    fn try_finish_apply_rejects_out_of_order() {
        let mut m = sealed(6, 2, false);
        for sid in 0..2 {
            apply_shard_fully(&mut m, sid, 6);
        }
        // skip shard 0, present shard 1 first
        assert_eq!(m.try_finish_apply(&[1]), Result::Err(Error::MergeOrder));
    }

    #[test]
    fn advance_apply_wrong_phase_in_reducing() {
        let mut m = sealed(4, 1, true); // Reducing
        assert_eq!(m.advance_apply(0, 2), Result::Err(Error::WrongPhase));
    }

    #[test]
    fn begin_apply_requires_reduced() {
        let mut m = sealed(4, 1, true); // Reducing, not Reduced
        assert_eq!(m.begin_apply(), Result::Err(Error::WrongPhase));
    }

    #[test]
    fn advance_apply_rejects_out_of_range_shard() {
        let mut m = sealed(4, 1, false);
        assert_eq!(m.advance_apply(5, 2), Result::Err(Error::ShardIdOutOfRange));
    }

    #[test]
    fn advance_past_len_is_shard_complete() {
        let mut m = sealed(2, 1, false);
        apply_shard_fully(&mut m, 0, 2); // cursor == len
        assert_eq!(m.advance_apply(0, 2), Result::Err(Error::ShardComplete));
    }

    #[test]
    fn pure_step_guards() {
        assert_eq!(advance_step(0, 10, 4), Ok(4));
        assert_eq!(advance_step(8, 10, 4), Ok(2), "clamps to remaining");
        assert_eq!(advance_step(10, 10, 4), Result::Err(Error::ShardComplete));
        assert_eq!(skip_step(3, 4), Ok(()));
        assert_eq!(skip_step(4, 4), Result::Err(Error::ShardComplete));
        // reduce_step: order + completeness + epoch
        assert_eq!(reduce_step(2, 2, 7, 7, 5, 5), Ok(()));
        assert_eq!(
            reduce_step(2, 3, 7, 7, 5, 5),
            Result::Err(Error::MergeOrder)
        );
        assert_eq!(
            reduce_step(2, 2, 6, 7, 5, 5),
            Result::Err(Error::ShardNotComplete),
            "stale epoch"
        );
        assert_eq!(
            reduce_step(2, 2, 7, 7, 4, 5),
            Result::Err(Error::ShardNotComplete),
            "incomplete"
        );
        assert_eq!(apply_finish_step(0, 0, 9, 9, 3, 3), Ok(()));
        assert_eq!(
            apply_finish_step(0, 1, 9, 9, 3, 3),
            Result::Err(Error::MergeOrder)
        );
    }

    /// Exhaustive liveness check: every sealed effect (single-pass & two-phase,
    /// across many shapes) reaches Done by cranking, with exact exactly-once coverage.
    /// This is the §14 liveness + exactly-once property, exhaustively.
    #[test]
    fn liveness_every_sealed_effect_reaches_done() {
        for total in 1..=40u64 {
            for p in 1..=(total.min(10) as u32) {
                for two_phase in [false, true] {
                    let mut m = sealed(total, p, two_phase);
                    if two_phase {
                        for sid in 0..p {
                            reduce_shard_fully(&mut m, sid, 5);
                        }
                        m.reduce_shards(&ids(p)).unwrap();
                        assert_eq!(m.status, Status::Reduced);
                        m.begin_apply().unwrap();
                    }
                    for sid in 0..p {
                        apply_shard_fully(&mut m, sid, 5);
                    }
                    m.try_finish_apply(&ids(p)).unwrap();
                    assert_eq!(
                        m.status,
                        Status::Done,
                        "must reach Done (total={total} p={p} two_phase={two_phase})"
                    );
                    let covered: u64 = m.shards.iter().map(|s| s.apply_cursor).sum();
                    assert_eq!(
                        covered, total,
                        "exactly-once coverage (total={total} p={p} two_phase={two_phase})"
                    );
                    m.finalize().unwrap();
                    assert!(m.finalized);
                }
            }
        }
    }
}
