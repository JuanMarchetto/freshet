//! Anchor port of the freshet reference settler, for the cross-framework CU benchmark.
//!
//! Same byte layout (SPEC §2), same validation, same delegation to the verified
//! `freshet::state` guards as the Pinocchio version — so the measured difference is
//! Anchor's framework overhead (8-byte sighash dispatch + `Context` construction +
//! `UncheckedAccount` handling), not different work. Accounts are raw `UncheckedAccount`s
//! cast to the byte layout (Anchor's typed deserialization would change those bytes), and
//! the variable member set rides in `ctx.remaining_accounts`.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::pubkey::Pubkey as SPubkey;
use freshet::state::{advance_step, apply_finish_step, Status};

declare_id!("DtLXaBhEudsViGhmpRu8K1WrudJp2QCMCS98H7tzQF5y");

const DISC_EFFECT: &[u8; 8] = b"FRSHEFCT";
const DISC_SHARD: &[u8; 8] = b"FRSHSHRD";
const DISC_ESCROW: &[u8; 8] = b"FRSHESCR";
const DISC_MEMBER: &[u8; 8] = b"FRSHMMBR";
const EFFECT_LEN: usize = 296;
const SHARD_LEN: usize = 152;
const ESCROW_LEN: usize = 64;
const MEMBER_LEN: usize = 80; // 72 header+pad + 8 body
const MEMBER_BODY: usize = 72;

#[error_code]
pub enum E {
    WrongPhase,
    BadAccount,
    Unauthorized,
    ShardComplete,
    BountyUnderfunded,
    BadShardCount,
    ShardsIncomplete,
}

fn ru32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
fn ru64(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}
fn wu64(d: &mut [u8], o: usize, v: u64) {
    d[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
fn wu32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn map_guard(e: freshet::state::Error) -> Error {
    match e {
        freshet::state::Error::ShardComplete => E::ShardComplete.into(),
        _ => E::WrongPhase.into(),
    }
}

fn check(ai: &AccountInfo, disc: &[u8; 8], len: usize) -> Result<()> {
    require!(ai.owner == &crate::ID, E::BadAccount);
    let d = ai.try_borrow_data()?;
    require!(d.len() >= len && &d[0..8] == disc, E::BadAccount);
    Ok(())
}
fn vpda(key: &Pubkey, seeds: &[&[u8]]) -> Result<()> {
    let derived = SPubkey::create_program_address(seeds, &crate::ID).map_err(|_| E::BadAccount)?;
    require!(&derived == key, E::BadAccount);
    Ok(())
}

#[program]
pub mod freshet_anchor {
    use super::*;

    pub fn init_effect(
        ctx: Context<InitEffect>,
        flags: u8,
        shard_count: u32,
        delta: u64,
    ) -> Result<()> {
        require!(shard_count >= 1, E::BadShardCount);
        let e = &ctx.accounts.effect;
        require!(e.owner == &crate::ID, E::BadAccount);
        let mut d = e.try_borrow_mut_data()?;
        require!(d.len() >= EFFECT_LEN && d[0..8] == [0u8; 8], E::BadAccount);
        d[0..8].copy_from_slice(DISC_EFFECT);
        d[8] = 2; // version
        d[9] = Status::Pending as u8;
        d[11] = flags;
        wu32(&mut d, 14, shard_count);
        wu64(&mut d, 26, 1); // epoch = 1 (0 = never-applied sentinel)
        wu64(&mut d, 34, 0); // total
        d[146..154].copy_from_slice(&delta.to_le_bytes()); // params[0..8] = delta
        d[50..82].copy_from_slice(&ctx.accounts.authority.key().to_bytes());
        Ok(())
    }

    pub fn init_shards(
        ctx: Context<InitShards>,
        shard_id: u32,
        shard_bump: u8,
        escrow_bump: u8,
        bounty_per: u64,
    ) -> Result<()> {
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let mut ed = ctx.accounts.effect.try_borrow_mut_data()?;
        require!(
            ed[50..82] == ctx.accounts.authority.key().to_bytes()
                && ctx.accounts.authority.is_signer,
            E::Unauthorized
        );
        require!(ed[9] == Status::Pending as u8, E::WrongPhase);
        let ek = ctx.accounts.effect.key();
        vpda(
            &ctx.accounts.shard.key(),
            &[
                b"cas.s",
                ek.as_ref(),
                &shard_id.to_le_bytes(),
                &[shard_bump],
            ],
        )?;
        vpda(
            &ctx.accounts.escrow.key(),
            &[
                b"cas.e",
                ek.as_ref(),
                &shard_id.to_le_bytes(),
                &[escrow_bump],
            ],
        )?;
        {
            let s = &ctx.accounts.shard;
            require!(s.owner == &crate::ID, E::BadAccount);
            let mut sd = s.try_borrow_mut_data()?;
            require!(sd.len() >= SHARD_LEN && sd[0..8] == [0u8; 8], E::BadAccount);
            sd[0..8].copy_from_slice(DISC_SHARD);
            sd[8] = 1;
            sd[9] = shard_bump;
            wu32(&mut sd, 12, shard_id);
            sd[16..48].copy_from_slice(ek.as_ref());
            wu64(&mut sd, 48, 0); // epoch 0 → lazy-reset on first touch
        }
        {
            let x = &ctx.accounts.escrow;
            require!(x.owner == &crate::ID, E::BadAccount);
            let mut xd = x.try_borrow_mut_data()?;
            require!(
                xd.len() >= ESCROW_LEN && xd[0..8] == [0u8; 8],
                E::BadAccount
            );
            xd[0..8].copy_from_slice(DISC_ESCROW);
            xd[8] = 1;
            xd[9] = escrow_bump;
            xd[10..42].copy_from_slice(ek.as_ref());
            wu32(&mut xd, 42, shard_id);
            wu64(&mut xd, 46, bounty_per);
        }
        let created = ru32(&ed, 290) + 1;
        wu32(&mut ed, 290, created);
        Ok(())
    }

    pub fn enroll(ctx: Context<Enroll>, member_bump: u8) -> Result<()> {
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let mut ed = ctx.accounts.effect.try_borrow_mut_data()?;
        require!(
            ed[50..82] == ctx.accounts.authority.key().to_bytes()
                && ctx.accounts.authority.is_signer,
            E::Unauthorized
        );
        require!(ed[9] == Status::Pending as u8, E::WrongPhase);
        let index = ru64(&ed, 34);
        let ek = ctx.accounts.effect.key();
        vpda(
            &ctx.accounts.member.key(),
            &[b"cas.m", ek.as_ref(), &index.to_le_bytes(), &[member_bump]],
        )?;
        let m = &ctx.accounts.member;
        require!(m.owner == &crate::ID, E::BadAccount);
        let mut md = m.try_borrow_mut_data()?;
        require!(
            md.len() >= MEMBER_LEN && md[0..8] == [0u8; 8],
            E::BadAccount
        );
        md[0..8].copy_from_slice(DISC_MEMBER);
        md[8] = 1;
        md[9] = member_bump;
        md[10..42].copy_from_slice(ek.as_ref());
        wu64(&mut md, 42, index);
        wu64(&mut ed, 34, index + 1);
        Ok(())
    }

    pub fn seal(ctx: Context<Seal>) -> Result<()> {
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let mut ed = ctx.accounts.effect.try_borrow_mut_data()?;
        require!(
            ed[50..82] == ctx.accounts.authority.key().to_bytes()
                && ctx.accounts.authority.is_signer,
            E::Unauthorized
        );
        require!(ed[9] == Status::Pending as u8, E::WrongPhase);
        let p = ru32(&ed, 14) as u64;
        let total = ru64(&ed, 34);
        require!(p >= 1 && p <= total, E::BadShardCount);
        require!(ru32(&ed, 290) == p as u32, E::ShardsIncomplete);
        wu32(&mut ed, 18, 0); // shards_done
        ed[9] = Status::Applying as u8;
        Ok(())
    }

    /// The hot path. `ctx.remaining_accounts` holds the member accounts for the batch.
    pub fn advance_apply(ctx: Context<AdvanceApply>, batch: u8) -> Result<()> {
        let ek = ctx.accounts.effect.key();
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let ed = ctx.accounts.effect.try_borrow_data()?;
        require!(ed[9] == Status::Applying as u8, E::WrongPhase);
        let epoch = ru64(&ed, 26);
        let total = ru64(&ed, 34);
        let shard_count = ru32(&ed, 14) as u64;
        let delta = ru64(&ed, 146);

        check(&ctx.accounts.shard, DISC_SHARD, SHARD_LEN)?;
        let mut sd = ctx.accounts.shard.try_borrow_mut_data()?;
        require!(sd[16..48] == ek.to_bytes(), E::BadAccount);
        let sid = ru32(&sd, 12);
        require!((sid as u64) < shard_count, E::BadAccount);
        vpda(
            &ctx.accounts.shard.key(),
            &[b"cas.s", ek.as_ref(), &sid.to_le_bytes(), &[sd[9]]],
        )?;
        // lazy-reset prologue
        if ru64(&sd, 48) < epoch {
            wu64(&mut sd, 56, 0);
            wu64(&mut sd, 64, 0);
            wu64(&mut sd, 48, epoch);
        }
        let base = total / shard_count;
        let rem = total % shard_count;
        let len = base + if (sid as u64) < rem { 1 } else { 0 };
        let start = (sid as u64) * base + (sid as u64).min(rem);
        let cursor = ru64(&sd, 64);
        let n = advance_step(cursor, len, batch as u64).map_err(map_guard)?;
        require!(ctx.remaining_accounts.len() as u64 >= n, E::BadAccount);

        // escrow binding (before reading bounty_per)
        check(&ctx.accounts.escrow, DISC_ESCROW, ESCROW_LEN)?;
        let bounty_per = {
            let xd = ctx.accounts.escrow.try_borrow_data()?;
            require!(
                xd[10..42] == ek.to_bytes() && ru32(&xd, 42) == sid,
                E::BadAccount
            );
            vpda(
                &ctx.accounts.escrow.key(),
                &[b"cas.e", ek.as_ref(), &sid.to_le_bytes(), &[xd[9]]],
            )?;
            ru64(&xd, 46)
        };
        let floor = Rent::get()?.minimum_balance(ESCROW_LEN);
        let amount = n.checked_mul(bounty_per).ok_or(E::BountyUnderfunded)?;
        require!(
            ctx.accounts.escrow.lamports() >= floor + amount,
            E::BountyUnderfunded
        );

        for j in 0..n {
            let gi = start + cursor + j;
            let m = &ctx.remaining_accounts[j as usize];
            require!(m.owner == &crate::ID, E::BadAccount);
            let mut md = m.try_borrow_mut_data()?;
            require!(
                md.len() >= MEMBER_LEN && &md[0..8] == DISC_MEMBER,
                E::BadAccount
            );
            require!(
                md[10..42] == ek.to_bytes() && ru64(&md, 42) == gi,
                E::BadAccount
            );
            vpda(
                &m.key(),
                &[b"cas.m", ek.as_ref(), &gi.to_le_bytes(), &[md[9]]],
            )?;
            if ru64(&md, 58) >= epoch {
                continue;
            }
            let cur = ru64(&md, MEMBER_BODY);
            wu64(&mut md, MEMBER_BODY, cur.saturating_add(delta));
            wu64(&mut md, 58, epoch);
        }
        **ctx.accounts.escrow.try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.cranker.try_borrow_mut_lamports()? += amount;
        wu64(&mut sd, 64, cursor + n);
        Ok(())
    }

    pub fn try_finish_apply(ctx: Context<TryFinish>) -> Result<()> {
        let ek = ctx.accounts.effect.key();
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let mut ed = ctx.accounts.effect.try_borrow_mut_data()?;
        require!(ed[9] == Status::Applying as u8, E::WrongPhase);
        let epoch = ru64(&ed, 26);
        let total = ru64(&ed, 34);
        let shard_count = ru32(&ed, 14) as u64;
        check(&ctx.accounts.shard, DISC_SHARD, SHARD_LEN)?;
        let sd = ctx.accounts.shard.try_borrow_data()?;
        require!(sd[16..48] == ek.to_bytes(), E::BadAccount);
        let sid = ru32(&sd, 12);
        let base = total / shard_count;
        let rem = total % shard_count;
        let len = base + if (sid as u64) < rem { 1 } else { 0 };
        apply_finish_step(ru32(&ed, 18), sid, ru64(&sd, 48), epoch, ru64(&sd, 64), len)
            .map_err(map_guard)?;
        let done = ru32(&ed, 18) + 1;
        wu32(&mut ed, 18, done);
        if done as u64 == shard_count {
            ed[9] = Status::Done as u8;
        }
        Ok(())
    }

    pub fn finalize(ctx: Context<Finalize>) -> Result<()> {
        check(&ctx.accounts.effect, DISC_EFFECT, EFFECT_LEN)?;
        let mut ed = ctx.accounts.effect.try_borrow_mut_data()?;
        require!(ed[9] == Status::Done as u8, E::WrongPhase);
        ed[11] |= 1 << 5; // FINALIZED
        Ok(())
    }
}

// All accounts are UncheckedAccount: freshet works on the raw byte layout, not Anchor's
// typed deserialization. /// CHECK: validated manually in the handlers (owner/disc/PDA).
#[derive(Accounts)]
pub struct InitEffect<'info> {
    /// CHECK: validated in-handler
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
    pub authority: Signer<'info>,
}
#[derive(Accounts)]
pub struct InitShards<'info> {
    /// CHECK:
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub shard: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub escrow: UncheckedAccount<'info>,
    pub authority: Signer<'info>,
}
#[derive(Accounts)]
pub struct Enroll<'info> {
    /// CHECK:
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub member: UncheckedAccount<'info>,
    pub authority: Signer<'info>,
}
#[derive(Accounts)]
pub struct Seal<'info> {
    /// CHECK:
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
    pub authority: Signer<'info>,
}
#[derive(Accounts)]
pub struct AdvanceApply<'info> {
    /// CHECK:
    pub effect: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub shard: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub escrow: UncheckedAccount<'info>,
    /// CHECK:
    #[account(mut)]
    pub cranker: Signer<'info>,
}
#[derive(Accounts)]
pub struct TryFinish<'info> {
    /// CHECK:
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
    /// CHECK:
    pub shard: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct Finalize<'info> {
    /// CHECK:
    #[account(mut)]
    pub effect: UncheckedAccount<'info>,
}
