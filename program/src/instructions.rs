//! Instruction handlers. Each validates accounts (§11 via `security`), reads/writes the
//! Pod layouts (§2), and DELEGATES every transition decision to the verified core
//! (`freshet::state` guards). This reference settler uses a concrete consumer: each
//! member body is a little-endian `u64` at `MEMBER_BODY_OFFSET`, and `apply` saturating-
//! adds `params[0..8]` (a "credit every account by delta" effect). Member/Shard/Escrow/
//! Effect accounts are pre-allocated program-owned (a production consumer would create
//! them via a System CPI; that is the only addition needed for on-chain allocation).

use crate::error::{custom, map, E_APPLY_FAILED, E_INDEX_MISMATCH, E_SLOT_OCCUPIED};
use crate::layout::{
    Effect, Escrow, MemberHeader, Shard, DISC_EFFECT, DISC_ESCROW, DISC_MEMBER, DISC_SHARD,
    FLAG_PULL, FLAG_REQUIRES_REDUCE, MEMBER_BODY_OFFSET, VERSION_EFFECT, VERSION_ESCROW,
    VERSION_MEMBER, VERSION_SHARD,
};
use crate::security::{
    authorize, check_header, pay_amount, rent_floor, transfer_lamports, verify_pda,
};
use crate::CONSUMER_ID;
use freshet::monoid::{Monoid, Sum};
use freshet::partition::{partition_len, partition_start};
use freshet::state::{advance_step, apply_finish_step, reduce_step, skip_step, Status};
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};

const MEMBER_BODY: usize = 8; // demo consumer: u64
const MEMBER_LEN: usize = MEMBER_BODY_OFFSET + MEMBER_BODY;

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[0..8]);
    u64::from_le_bytes(a)
}

/// init_effect: data = [bump(1), flags(1), shard_count(4), params(128)].
/// Accounts: [effect(w, pre-alloc 296, zeroed), authority(signer)].
pub fn init_effect(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [effect_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.len() < 1 + 1 + 4 + 128 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bump = data[0];
    let flags = data[1];
    let shard_count = le_u32(&data[2..6]);
    if flags & crate::layout::FLAG_ACC_EXTERNAL != 0 {
        return Err(custom(crate::error::E_ACC_EXTERNAL_DEFERRED));
    }
    if shard_count < 1 || (shard_count > 1 && flags & crate::layout::FLAG_ORDER_INDEPENDENT == 0) {
        return Err(map(freshet::state::Error::NotShardable));
    }
    if !authority_ai.is_signer() {
        return Err(custom(crate::error::E_UNAUTHORIZED));
    }
    let mut edata = effect_ai.try_borrow_mut_data()?;
    if edata.len() < Effect::LEN {
        return Err(custom(crate::error::E_ACCOUNT_DATA_TOO_SMALL));
    }
    if edata[0..8] != [0u8; 8] {
        return Err(custom(E_SLOT_OCCUPIED)); // already initialized
    }
    if !effect_ai.is_owned_by(&CONSUMER_ID) {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    e.disc = DISC_EFFECT;
    e.version = VERSION_EFFECT;
    e.bump = bump;
    e.flags = flags;
    e.set_status(Status::Pending);
    e.set_epoch(1); // epoch 0 is the never-applied sentinel (see freshet::state::Machine::init)
    e.acc_global = [0u8; 64];
    e.shard_count = shard_count.to_le_bytes();
    e.total = 0u64.to_le_bytes();
    e.shards_done = 0u32.to_le_bytes();
    e.set_merge_cursor(0);
    e.reduce_skipped_count = 0u64.to_le_bytes();
    e.apply_skipped_count = 0u64.to_le_bytes();
    e.shards_created = 0u32.to_le_bytes();
    e.params.copy_from_slice(&data[6..134]);
    e.authority = *authority_ai.key();
    Ok(())
}

/// init_shards: data = [shard_id(4), shard_bump(1), escrow_bump(1), bounty_per(8)].
/// Accounts: [effect(w), shard(w, pre-alloc 152 zeroed), escrow(w, pre-alloc 64 zeroed), authority(signer)].
pub fn init_shards(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [effect_ai, shard_ai, escrow_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.len() < 4 + 1 + 1 + 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let sid = le_u32(&data[0..4]);
    let shard_bump = data[4];
    let escrow_bump = data[5];
    let bounty_per = le_u64(&data[6..14]);

    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    if e.status().map_err(map)? != Status::Pending {
        return Err(map(freshet::state::Error::WrongPhase));
    }
    verify_pda(
        shard_ai.key(),
        &[b"cas.s", effect_ai.key(), &sid.to_le_bytes(), &[shard_bump]],
        &CONSUMER_ID,
    )?;
    verify_pda(
        escrow_ai.key(),
        &[
            b"cas.e",
            effect_ai.key(),
            &sid.to_le_bytes(),
            &[escrow_bump],
        ],
        &CONSUMER_ID,
    )?;
    {
        let mut sdata = shard_ai.try_borrow_mut_data()?;
        if sdata.len() < Shard::LEN || !shard_ai.is_owned_by(&CONSUMER_ID) {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        if sdata[0..8] != [0u8; 8] {
            return Err(custom(E_SLOT_OCCUPIED));
        }
        let s: &mut Shard = unsafe { &mut *(sdata.as_mut_ptr() as *mut Shard) };
        s.disc = DISC_SHARD;
        s.version = VERSION_SHARD;
        s.bump = shard_bump;
        s.shard_id = sid.to_le_bytes();
        s.effect = *effect_ai.key();
        s.set_epoch(0); // stale vs effect.epoch=1 ⇒ lazy-reset on first touch
        s.set_reduce_cursor(0);
        s.set_apply_cursor(0);
        s.acc_partial = [0u8; 64]; // IDENTITY (Sum demo)
    }
    {
        let mut xdata = escrow_ai.try_borrow_mut_data()?;
        if xdata.len() < Escrow::LEN || !escrow_ai.is_owned_by(&CONSUMER_ID) {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        if xdata[0..8] != [0u8; 8] {
            return Err(custom(E_SLOT_OCCUPIED));
        }
        let x: &mut Escrow = unsafe { &mut *(xdata.as_mut_ptr() as *mut Escrow) };
        x.disc = DISC_ESCROW;
        x.version = VERSION_ESCROW;
        x.bump = escrow_bump;
        x.effect = *effect_ai.key();
        x.shard_id = sid.to_le_bytes();
        x.bounty_per = bounty_per.to_le_bytes();
        x.last_refund_epoch = 0u64.to_le_bytes();
    }
    e.shards_created = (e.shards_created() + 1).to_le_bytes();
    Ok(())
}

/// enroll: data = [member_bump(1)]. Accounts: [effect(w), member(w, pre-alloc, zeroed), authority(signer)].
pub fn enroll(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [effect_ai, member_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let member_bump = data[0];
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    if e.status().map_err(map)? != Status::Pending {
        return Err(map(freshet::state::Error::WrongPhase));
    }
    let index = e.total();
    verify_pda(
        member_ai.key(),
        &[
            b"cas.m",
            effect_ai.key(),
            &index.to_le_bytes(),
            &[member_bump],
        ],
        &CONSUMER_ID,
    )?;
    let mut mdata = member_ai.try_borrow_mut_data()?;
    if mdata.len() < MEMBER_LEN || !member_ai.is_owned_by(&CONSUMER_ID) {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    if mdata[0..8] != [0u8; 8] {
        return Err(custom(E_SLOT_OCCUPIED));
    }
    let h: &mut MemberHeader = unsafe { &mut *(mdata.as_mut_ptr() as *mut MemberHeader) };
    h.disc = DISC_MEMBER;
    h.version = VERSION_MEMBER;
    h.bump = member_bump;
    h.effect = *effect_ai.key();
    h.index = index.to_le_bytes();
    h.set_last_reduce_epoch(0);
    h.set_last_apply_epoch(0);
    e.total = (index + 1).to_le_bytes();
    Ok(())
}

/// seal: []. Accounts: [effect(w), authority(signer)]. Single-pass demo ⇒ → Applying.
pub fn seal(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Pending {
        return Err(map(CE::WrongPhase));
    }
    if e.flags & FLAG_PULL != 0 {
        return Err(map(CE::PullPushExclusive));
    }
    if e.epoch() < 1 {
        return Err(map(CE::EpochUninit));
    }
    let p = e.shard_count() as u64;
    if !(p >= 1 && p <= e.total()) {
        return Err(map(CE::BadShardCount));
    }
    if e.shards_created() != e.shard_count() {
        return Err(map(CE::ShardsIncomplete));
    }
    e.set_shards_done(0);
    let next = if e.flags & FLAG_REQUIRES_REDUCE != 0 {
        Status::Reducing
    } else {
        Status::Applying
    };
    e.set_status(next);
    Ok(())
}

/// advance_apply: data = [batch(1)]. Accounts:
/// [effect(ro), shard(w), escrow(w), cranker(signer,w), member_0(w) .. member_{batch-1}(w)].
pub fn advance_apply(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let batch = data[0] as u64;
    let [effect_ai, shard_ai, escrow_ai, cranker_ai, members @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    // Effect read-only (hot path never writes it — restores Sealevel parallelism).
    let edata = effect_ai.try_borrow_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &Effect = unsafe { &*(edata.as_ptr() as *const Effect) };
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Applying {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    let delta = le_u64(&e.params[0..8]);

    let mut sdata = shard_ai.try_borrow_mut_data()?;
    check_header(
        shard_ai,
        &CONSUMER_ID,
        &sdata,
        &DISC_SHARD,
        VERSION_SHARD,
        Shard::LEN,
    )?;
    let s: &mut Shard = unsafe { &mut *(sdata.as_mut_ptr() as *mut Shard) };
    if s.effect != *effect_ai.key() {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let sid = s.shard_id();
    if sid >= e.shard_count() {
        return Err(map(CE::ShardIdOutOfRange));
    }
    verify_pda(
        shard_ai.key(),
        &[b"cas.s", effect_ai.key(), &sid.to_le_bytes(), &[s.bump]],
        &CONSUMER_ID,
    )?;
    s.lazy_reset(epoch);
    let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
    let start = partition_start(e.total(), e.shard_count() as u64, sid as u64);
    let cursor = s.apply_cursor();
    let n = advance_step(cursor, len, batch).map_err(map)?;
    if (members.len() as u64) < n {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // Bind the escrow to (effect, shard_id) before reading bounty_per — otherwise a
    // crank could pass a fatter, unrelated escrow and drain it.
    let bounty_per = {
        let xdata = escrow_ai.try_borrow_data()?;
        check_header(
            escrow_ai,
            &CONSUMER_ID,
            &xdata,
            &DISC_ESCROW,
            VERSION_ESCROW,
            Escrow::LEN,
        )?;
        let x: &Escrow = unsafe { &*(xdata.as_ptr() as *const Escrow) };
        if x.effect != *effect_ai.key() || x.shard_id() != sid {
            return Err(map(CE::ShardIdOutOfRange));
        }
        verify_pda(
            escrow_ai.key(),
            &[b"cas.e", effect_ai.key(), &sid.to_le_bytes(), &[x.bump]],
            &CONSUMER_ID,
        )?;
        x.bounty_per()
    };
    let floor = rent_floor(Escrow::LEN)?;
    let amount = pay_amount(escrow_ai.lamports(), floor, n, bounty_per)?; // all-or-nothing FIRST

    // Member loop: per-slot verify + apply + cross-epoch idempotency stamp.
    for j in 0..n {
        let gi = start + cursor + j;
        let m = &members[j as usize];
        let mut mdata = m.try_borrow_mut_data()?;
        if mdata.len() < MEMBER_LEN || !m.is_owned_by(&CONSUMER_ID) {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        let (hbytes, body) = mdata.split_at_mut(MEMBER_BODY_OFFSET);
        if hbytes[0..8] != DISC_MEMBER {
            return Err(custom(crate::error::E_BAD_DISCRIMINATOR));
        }
        let h: &mut MemberHeader = unsafe { &mut *(hbytes.as_mut_ptr() as *mut MemberHeader) };
        if h.effect != *effect_ai.key() || h.index() != gi {
            return Err(custom(E_INDEX_MISMATCH));
        }
        verify_pda(
            m.key(),
            &[b"cas.m", effect_ai.key(), &gi.to_le_bytes(), &[h.bump]],
            &CONSUMER_ID,
        )?;
        if h.last_apply_epoch() >= epoch {
            continue; // §4 cross-epoch idempotency
        }
        // C::apply — demo: saturating credit.
        let cur = le_u64(&body[0..8]);
        let new = cur.checked_add(delta).ok_or(custom(E_APPLY_FAILED))?;
        body[0..8].copy_from_slice(&new.to_le_bytes());
        h.set_last_apply_epoch(epoch);
    }
    // tail slots [n, members.len()) must be the readonly sentinel (== effect key).
    for m in &members[n as usize..] {
        if m.key() != effect_ai.key() {
            return Err(custom(crate::error::E_SENTINEL_EXPECTED));
        }
    }

    transfer_lamports(escrow_ai, cranker_ai, amount)?;
    s.set_apply_cursor(cursor + n);
    Ok(())
}

/// try_finish_apply: []. Accounts: [effect(w), shard_{shards_done} .. ascending].
pub fn try_finish_apply(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, shards @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Applying {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    for shard_ai in shards {
        let sdata = shard_ai.try_borrow_data()?;
        check_header(
            shard_ai,
            &CONSUMER_ID,
            &sdata,
            &DISC_SHARD,
            VERSION_SHARD,
            Shard::LEN,
        )?;
        let s: &Shard = unsafe { &*(sdata.as_ptr() as *const Shard) };
        if s.effect != *effect_ai.key() {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        let sid = s.shard_id();
        let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
        apply_finish_step(
            e.shards_done(),
            sid,
            s.epoch(),
            epoch,
            s.apply_cursor(),
            len,
        )
        .map_err(map)?;
        e.set_shards_done(e.shards_done() + 1);
    }
    if e.shards_done() == e.shard_count() {
        e.set_status(Status::Done);
    }
    Ok(())
}

/// finalize: []. Accounts: [effect(w)].
pub fn finalize(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    if e.status().map_err(map)? != Status::Done {
        return Err(map(freshet::state::Error::WrongPhase));
    }
    if e.flags & crate::layout::FLAG_FINALIZED != 0 {
        return Err(map(freshet::state::Error::AlreadyFinalized));
    }
    e.flags |= crate::layout::FLAG_FINALIZED;
    Ok(())
}

/// skip: []. Accounts: [effect(w for skipped_count), shard(w), authority(signer), member(w)].
pub fn skip(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, shard_ai, authority_ai, _member_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Applying {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    let mut sdata = shard_ai.try_borrow_mut_data()?;
    check_header(
        shard_ai,
        &CONSUMER_ID,
        &sdata,
        &DISC_SHARD,
        VERSION_SHARD,
        Shard::LEN,
    )?;
    let s: &mut Shard = unsafe { &mut *(sdata.as_mut_ptr() as *mut Shard) };
    if s.effect != *effect_ai.key() {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let sid = s.shard_id();
    s.lazy_reset(epoch);
    let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
    skip_step(s.apply_cursor(), len).map_err(map)?;
    s.set_apply_cursor(s.apply_cursor() + 1);
    e.apply_skipped_count = (le_u64(&e.apply_skipped_count) + 1).to_le_bytes();
    Ok(())
}

/// advance_reduce: data=[batch(1)]. Accounts: [effect(ro), shard(w), escrow(w),
/// cranker(signer,w), member_0(w)..]. Folds member values into acc_partial (Sum demo).
pub fn advance_reduce(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let batch = data[0] as u64;
    let [effect_ai, shard_ai, escrow_ai, cranker_ai, members @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let edata = effect_ai.try_borrow_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &Effect = unsafe { &*(edata.as_ptr() as *const Effect) };
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Reducing {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    let mut sdata = shard_ai.try_borrow_mut_data()?;
    check_header(
        shard_ai,
        &CONSUMER_ID,
        &sdata,
        &DISC_SHARD,
        VERSION_SHARD,
        Shard::LEN,
    )?;
    let s: &mut Shard = unsafe { &mut *(sdata.as_mut_ptr() as *mut Shard) };
    if s.effect != *effect_ai.key() {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let sid = s.shard_id();
    if sid >= e.shard_count() {
        return Err(map(CE::ShardIdOutOfRange));
    }
    verify_pda(
        shard_ai.key(),
        &[b"cas.s", effect_ai.key(), &sid.to_le_bytes(), &[s.bump]],
        &CONSUMER_ID,
    )?;
    s.lazy_reset(epoch);
    let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
    let start = partition_start(e.total(), e.shard_count() as u64, sid as u64);
    let cursor = s.reduce_cursor();
    let n = advance_step(cursor, len, batch).map_err(map)?;
    if (members.len() as u64) < n {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let bounty_per = {
        let xdata = escrow_ai.try_borrow_data()?;
        check_header(
            escrow_ai,
            &CONSUMER_ID,
            &xdata,
            &DISC_ESCROW,
            VERSION_ESCROW,
            Escrow::LEN,
        )?;
        let x: &Escrow = unsafe { &*(xdata.as_ptr() as *const Escrow) };
        if x.effect != *effect_ai.key() || x.shard_id() != sid {
            return Err(map(CE::ShardIdOutOfRange));
        }
        verify_pda(
            escrow_ai.key(),
            &[b"cas.e", effect_ai.key(), &sid.to_le_bytes(), &[x.bump]],
            &CONSUMER_ID,
        )?;
        x.bounty_per()
    };
    let floor = rent_floor(Escrow::LEN)?;
    let amount = pay_amount(escrow_ai.lamports(), floor, n, bounty_per)?;
    let mut partial = Sum::from_acc_bytes(&s.acc_partial);
    for j in 0..n {
        let gi = start + cursor + j;
        let m = &members[j as usize];
        let mut mdata = m.try_borrow_mut_data()?;
        if mdata.len() < MEMBER_LEN || !m.is_owned_by(&CONSUMER_ID) {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        let (hbytes, body) = mdata.split_at_mut(MEMBER_BODY_OFFSET);
        if hbytes[0..8] != DISC_MEMBER {
            return Err(custom(crate::error::E_BAD_DISCRIMINATOR));
        }
        let h: &mut MemberHeader = unsafe { &mut *(hbytes.as_mut_ptr() as *mut MemberHeader) };
        if h.effect != *effect_ai.key() || h.index() != gi {
            return Err(custom(E_INDEX_MISMATCH));
        }
        verify_pda(
            m.key(),
            &[b"cas.m", effect_ai.key(), &gi.to_le_bytes(), &[h.bump]],
            &CONSUMER_ID,
        )?;
        if h.last_reduce_epoch() >= epoch {
            continue;
        }
        partial = partial.combine(Sum(le_u64(&body[0..8])));
        h.set_last_reduce_epoch(epoch);
    }
    for m in &members[n as usize..] {
        if m.key() != effect_ai.key() {
            return Err(custom(crate::error::E_SENTINEL_EXPECTED));
        }
    }
    s.acc_partial = partial.to_acc_bytes();
    transfer_lamports(escrow_ai, cranker_ai, amount)?;
    s.set_reduce_cursor(cursor + n);
    Ok(())
}

/// reduce_shards: []. Accounts: [effect(w), shard_{merge_cursor}.. ascending]. Merges
/// each done shard's acc_partial into acc_global (Sum), in order.
pub fn reduce_shards(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, shards @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Reducing {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    for shard_ai in shards {
        let sdata = shard_ai.try_borrow_data()?;
        check_header(
            shard_ai,
            &CONSUMER_ID,
            &sdata,
            &DISC_SHARD,
            VERSION_SHARD,
            Shard::LEN,
        )?;
        let s: &Shard = unsafe { &*(sdata.as_ptr() as *const Shard) };
        if s.effect != *effect_ai.key() {
            return Err(custom(crate::error::E_BAD_OWNER));
        }
        let sid = s.shard_id();
        let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
        reduce_step(
            e.merge_cursor(),
            sid,
            s.epoch(),
            epoch,
            s.reduce_cursor(),
            len,
        )
        .map_err(map)?;
        let merged =
            Sum::from_acc_bytes(&e.acc_global).combine(Sum::from_acc_bytes(&s.acc_partial));
        e.acc_global = merged.to_acc_bytes();
        e.set_merge_cursor(e.merge_cursor() + 1);
    }
    if e.merge_cursor() == e.shard_count() {
        e.set_status(Status::Reduced);
    }
    Ok(())
}

/// begin_apply: []. Accounts: [effect(w), authority? permissionless]. Reduced→Applying.
pub fn begin_apply(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Reduced {
        return Err(map(CE::WrongPhase));
    }
    if e.flags & FLAG_PULL != 0 {
        return Err(map(CE::PullPushExclusive));
    }
    e.set_shards_done(0);
    e.set_status(Status::Applying);
    Ok(())
}

/// skip_reduce: []. Accounts: [effect(w), shard(w), authority(signer), member(w)].
pub fn skip_reduce(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, shard_ai, authority_ai, _member_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    use freshet::state::Error as CE;
    if e.status().map_err(map)? != Status::Reducing {
        return Err(map(CE::WrongPhase));
    }
    let epoch = e.epoch();
    let mut sdata = shard_ai.try_borrow_mut_data()?;
    check_header(
        shard_ai,
        &CONSUMER_ID,
        &sdata,
        &DISC_SHARD,
        VERSION_SHARD,
        Shard::LEN,
    )?;
    let s: &mut Shard = unsafe { &mut *(sdata.as_mut_ptr() as *mut Shard) };
    if s.effect != *effect_ai.key() {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let sid = s.shard_id();
    s.lazy_reset(epoch);
    let len = partition_len(e.total(), e.shard_count() as u64, sid as u64);
    skip_step(s.reduce_cursor(), len).map_err(map)?;
    s.set_reduce_cursor(s.reduce_cursor() + 1);
    e.reduce_skipped_count = (le_u64(&e.reduce_skipped_count) + 1).to_le_bytes();
    Ok(())
}

/// cancel: []. Accounts: [effect(w), authority(signer)].
pub fn cancel(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    let st = e.status().map_err(map)?;
    if st == Status::Done || st == Status::Cancelled {
        return Err(map(freshet::state::Error::WrongPhase));
    }
    e.set_status(Status::Cancelled);
    Ok(())
}

/// reset: re-run a finished effect. Only an effect that was sealable (1<=shard_count<=total,
/// every shard created) is resettable, so a cancel before the first seal cannot be re-run.
pub fn reset(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let mut edata = effect_ai.try_borrow_mut_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &mut Effect = unsafe { &mut *(edata.as_mut_ptr() as *mut Effect) };
    authorize(authority_ai, &e.authority)?;
    use freshet::state::Error as CE;
    let st = e.status().map_err(map)?;
    if st != Status::Done && st != Status::Cancelled {
        return Err(map(CE::WrongPhase));
    }
    if e.flags & FLAG_PULL != 0 {
        return Err(map(CE::PullNoReset));
    }
    let p = e.shard_count() as u64;
    if !(p >= 1 && p <= e.total()) || e.shards_created() != e.shard_count() {
        return Err(map(CE::NotSealed)); // a cancel before the first seal is not resettable
    }
    e.set_epoch(e.epoch() + 1);
    e.set_shards_done(0);
    e.set_merge_cursor(0);
    e.acc_global = [0u8; 64];
    e.reduce_skipped_count = 0u64.to_le_bytes();
    e.apply_skipped_count = 0u64.to_le_bytes();
    e.flags &= !crate::layout::FLAG_FINALIZED;
    let next = if e.flags & FLAG_REQUIRES_REDUCE != 0 {
        Status::Reducing
    } else {
        Status::Applying
    };
    e.set_status(next);
    Ok(())
}

/// refund_escrow: []. Accounts: [effect(ro), escrow(w), authority(w, receives)].
/// Sweeps surplus above the rent floor to the authority; NEVER closes; one per epoch.
pub fn refund_escrow(accounts: &[AccountInfo]) -> ProgramResult {
    let [effect_ai, escrow_ai, authority_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let edata = effect_ai.try_borrow_data()?;
    check_header(
        effect_ai,
        &CONSUMER_ID,
        &edata,
        &DISC_EFFECT,
        VERSION_EFFECT,
        Effect::LEN,
    )?;
    let e: &Effect = unsafe { &*(edata.as_ptr() as *const Effect) };
    let st = e.status().map_err(map)?;
    if st != Status::Done && st != Status::Cancelled {
        return Err(map(freshet::state::Error::WrongPhase));
    }
    // Permissionless: anyone may trigger a refund, but the destination is pinned to the
    // stored authority, so no signer is required and nothing can be misdirected.
    if authority_ai.key() != &e.authority {
        return Err(custom(crate::error::E_UNAUTHORIZED));
    }
    let epoch = e.epoch();
    let mut xdata = escrow_ai.try_borrow_mut_data()?;
    check_header(
        escrow_ai,
        &CONSUMER_ID,
        &xdata,
        &DISC_ESCROW,
        VERSION_ESCROW,
        Escrow::LEN,
    )?;
    let x: &mut Escrow = unsafe { &mut *(xdata.as_mut_ptr() as *mut Escrow) };
    if x.effect != *effect_ai.key() {
        return Err(custom(crate::error::E_BAD_OWNER));
    }
    let sid = x.shard_id();
    verify_pda(
        escrow_ai.key(),
        &[b"cas.e", effect_ai.key(), &sid.to_le_bytes(), &[x.bump]],
        &CONSUMER_ID,
    )?;
    if x.last_refund_epoch() >= epoch {
        return Ok(()); // already refunded this epoch (idempotent no-op)
    }
    x.set_last_refund_epoch(epoch);
    let floor = rent_floor(Escrow::LEN)?;
    let surplus = escrow_ai.lamports().saturating_sub(floor);
    if surplus > 0 {
        transfer_lamports(escrow_ai, authority_ai, surplus)?;
    }
    Ok(())
}

/// top_up_bounty: data=[amount(8)]. Accounts: [escrow(w), payer(signer,w)].
pub fn top_up_bounty(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [escrow_ai, payer_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.len() < 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = le_u64(&data[0..8]);
    let xdata = escrow_ai.try_borrow_data()?;
    check_header(
        escrow_ai,
        &CONSUMER_ID,
        &xdata,
        &DISC_ESCROW,
        VERSION_ESCROW,
        Escrow::LEN,
    )?;
    drop(xdata);
    if !payer_ai.is_signer() {
        return Err(custom(crate::error::E_UNAUTHORIZED));
    }
    transfer_lamports(payer_ai, escrow_ai, amount)?;
    Ok(())
}
