//! Quasar port of the freshet reference settler, for the cross-framework CU benchmark.
//!
//! Same byte layout (SPEC §2), same validation, and the same delegation to the verified
//! `freshet::state` guards as the Pinocchio and Anchor versions — so the measured
//! difference is attributable to the framework (1-byte discriminator dispatch + `Ctx`
//! construction + zero-copy account parsing + PDA-verification primitive), not different
//! work. Accounts are raw
//! `UncheckedAccount`s operated on at the byte level (Quasar's typed `Account<T>` would
//! impose its own discriminator/layout), and the variable member set rides in the
//! remaining accounts.

#![no_std]

use freshet::state::{advance_step, apply_finish_step, Status};
use quasar_lang::pda::verify_program_address;
use quasar_lang::prelude::*;
use quasar_lang::sysvars::Sysvar;

declare_id!("4EVuHUEhrhjf2SEvy6aFR8Zpq2CjSfnMccNcyLFGa8qU");

const DISC_EFFECT: &[u8; 8] = b"FRSHEFCT";
const DISC_SHARD: &[u8; 8] = b"FRSHSHRD";
const DISC_ESCROW: &[u8; 8] = b"FRSHESCR";
const DISC_MEMBER: &[u8; 8] = b"FRSHMMBR";
const EFFECT_LEN: usize = 296;
const SHARD_LEN: usize = 152;
const ESCROW_LEN: usize = 64;
const MEMBER_LEN: usize = 80; // 72 header+pad + 8 body
const MEMBER_BODY: usize = 72;

// Custom error codes (the cross-framework benchmark compares CU on identical work, so the
// exact numbering only needs to be internally consistent).
const E_WRONG_PHASE: u32 = 0;
const E_BAD_ACCOUNT: u32 = 1;
const E_UNAUTHORIZED: u32 = 2;
const E_SHARD_COMPLETE: u32 = 3;
const E_BOUNTY_UNDERFUNDED: u32 = 4;
const E_BAD_SHARD_COUNT: u32 = 5;
const E_SHARDS_INCOMPLETE: u32 = 6;

#[inline(always)]
fn err(code: u32) -> ProgramError {
    ProgramError::Custom(code)
}

fn map_guard(e: freshet::state::Error) -> ProgramError {
    match e {
        freshet::state::Error::ShardComplete => err(E_SHARD_COMPLETE),
        _ => err(E_WRONG_PHASE),
    }
}

#[inline(always)]
fn ru32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
#[inline(always)]
fn ru64(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}
#[inline(always)]
fn wu64(d: &mut [u8], o: usize, v: u64) {
    d[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// Owner + discriminator + length check on a program-owned account.
fn check(view: &AccountView, disc: &[u8; 8], len: usize) -> Result<(), ProgramError> {
    require!(view.owned_by(&crate::ID), err(E_BAD_ACCOUNT));
    let d = view.try_borrow()?;
    require!(d.len() >= len && &d[0..8] == disc, err(E_BAD_ACCOUNT));
    Ok(())
}

/// Verify that `view`'s address is the PDA for `seeds` (which include the bump).
fn vpda(view: &AccountView, seeds: &[&[u8]]) -> Result<(), ProgramError> {
    verify_program_address(seeds, &crate::ID, view.address()).map_err(|_| err(E_BAD_ACCOUNT))
}
fn vpda_addr(addr: &Address, seeds: &[&[u8]]) -> Result<(), ProgramError> {
    verify_program_address(seeds, &crate::ID, addr).map_err(|_| err(E_BAD_ACCOUNT))
}

#[program]
mod freshet_quasar {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn init_effect(
        ctx: Ctx<InitEffect>,
        flags: u8,
        shard_count: u32,
        delta: u64,
    ) -> Result<(), ProgramError> {
        require!(shard_count >= 1, err(E_BAD_SHARD_COUNT));
        {
            let v = ctx.accounts.effect.to_account_view();
            require!(v.owned_by(&crate::ID), err(E_BAD_ACCOUNT));
            let d = v.try_borrow()?;
            require!(
                d.len() >= EFFECT_LEN && d[0..8] == [0u8; 8],
                err(E_BAD_ACCOUNT)
            );
        }
        let auth = *ctx.accounts.authority.to_account_view().address();
        let e = &mut ctx.accounts.effect;
        e.write_bytes(0, DISC_EFFECT)?;
        e.write_bytes(8, &[2])?; // version
        e.write_bytes(9, &[Status::Pending as u8])?;
        e.write_bytes(11, &[flags])?;
        e.write_bytes(14, &shard_count.to_le_bytes())?;
        e.write_bytes(26, &1u64.to_le_bytes())?; // epoch = 1 (0 = never-applied sentinel)
        e.write_bytes(34, &0u64.to_le_bytes())?; // total
        e.write_bytes(146, &delta.to_le_bytes())?; // params[0..8] = delta
        e.write_bytes(50, auth.as_ref())?;
        Ok(())
    }

    #[instruction(discriminator = 1)]
    pub fn init_shards(
        ctx: Ctx<InitShards>,
        shard_id: u32,
        shard_bump: u8,
        escrow_bump: u8,
        bounty_per: u64,
    ) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        let ek = *ctx.accounts.effect.to_account_view().address();
        let created = {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(
                &d[50..82] == ctx.accounts.authority.to_account_view().address().as_ref(),
                err(E_UNAUTHORIZED)
            );
            require!(d[9] == Status::Pending as u8, err(E_WRONG_PHASE));
            ru32(&d, 290)
        };
        vpda(
            ctx.accounts.shard.to_account_view(),
            &[
                b"cas.s",
                ek.as_ref(),
                &shard_id.to_le_bytes(),
                &[shard_bump],
            ],
        )?;
        vpda(
            ctx.accounts.escrow.to_account_view(),
            &[
                b"cas.e",
                ek.as_ref(),
                &shard_id.to_le_bytes(),
                &[escrow_bump],
            ],
        )?;
        {
            let v = ctx.accounts.shard.to_account_view();
            require!(v.owned_by(&crate::ID), err(E_BAD_ACCOUNT));
            let d = v.try_borrow()?;
            require!(
                d.len() >= SHARD_LEN && d[0..8] == [0u8; 8],
                err(E_BAD_ACCOUNT)
            );
        }
        {
            let s = &mut ctx.accounts.shard;
            s.write_bytes(0, DISC_SHARD)?;
            s.write_bytes(8, &[1])?; // version
            s.write_bytes(9, &[shard_bump])?;
            s.write_bytes(12, &shard_id.to_le_bytes())?;
            s.write_bytes(16, ek.as_ref())?;
            s.write_bytes(48, &0u64.to_le_bytes())?; // epoch 0 → lazy-reset on first touch
        }
        {
            let v = ctx.accounts.escrow.to_account_view();
            require!(v.owned_by(&crate::ID), err(E_BAD_ACCOUNT));
            let d = v.try_borrow()?;
            require!(
                d.len() >= ESCROW_LEN && d[0..8] == [0u8; 8],
                err(E_BAD_ACCOUNT)
            );
        }
        {
            let x = &mut ctx.accounts.escrow;
            x.write_bytes(0, DISC_ESCROW)?;
            x.write_bytes(8, &[1])?; // version
            x.write_bytes(9, &[escrow_bump])?;
            x.write_bytes(10, ek.as_ref())?;
            x.write_bytes(42, &shard_id.to_le_bytes())?;
            x.write_bytes(46, &bounty_per.to_le_bytes())?;
        }
        ctx.accounts
            .effect
            .write_bytes(290, &(created + 1).to_le_bytes())?;
        Ok(())
    }

    #[instruction(discriminator = 2)]
    pub fn enroll(ctx: Ctx<Enroll>, member_bump: u8) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        let ek = *ctx.accounts.effect.to_account_view().address();
        let index = {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(
                &d[50..82] == ctx.accounts.authority.to_account_view().address().as_ref(),
                err(E_UNAUTHORIZED)
            );
            require!(d[9] == Status::Pending as u8, err(E_WRONG_PHASE));
            ru64(&d, 34)
        };
        vpda(
            ctx.accounts.member.to_account_view(),
            &[b"cas.m", ek.as_ref(), &index.to_le_bytes(), &[member_bump]],
        )?;
        {
            let v = ctx.accounts.member.to_account_view();
            require!(v.owned_by(&crate::ID), err(E_BAD_ACCOUNT));
            let d = v.try_borrow()?;
            require!(
                d.len() >= MEMBER_LEN && d[0..8] == [0u8; 8],
                err(E_BAD_ACCOUNT)
            );
        }
        {
            let m = &mut ctx.accounts.member;
            m.write_bytes(0, DISC_MEMBER)?;
            m.write_bytes(8, &[1])?; // version
            m.write_bytes(9, &[member_bump])?;
            m.write_bytes(10, ek.as_ref())?;
            m.write_bytes(42, &index.to_le_bytes())?;
        }
        ctx.accounts
            .effect
            .write_bytes(34, &(index + 1).to_le_bytes())?;
        Ok(())
    }

    #[instruction(discriminator = 3)]
    pub fn seal(ctx: Ctx<Seal>) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(
                &d[50..82] == ctx.accounts.authority.to_account_view().address().as_ref(),
                err(E_UNAUTHORIZED)
            );
            require!(d[9] == Status::Pending as u8, err(E_WRONG_PHASE));
            let p = ru32(&d, 14) as u64;
            let total = ru64(&d, 34);
            require!(p >= 1 && p <= total, err(E_BAD_SHARD_COUNT));
            require!(ru32(&d, 290) as u64 == p, err(E_SHARDS_INCOMPLETE));
        }
        let e = &mut ctx.accounts.effect;
        e.write_bytes(18, &0u32.to_le_bytes())?; // shards_done
        e.write_bytes(9, &[Status::Applying as u8])?;
        Ok(())
    }

    /// The hot path. The member accounts for the batch ride in the remaining accounts.
    #[instruction(discriminator = 4)]
    pub fn advance_apply(
        ctx: CtxWithRemaining<AdvanceApply>,
        batch: u8,
    ) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        let ek = *ctx.accounts.effect.to_account_view().address();
        let (epoch, total, shard_count, delta) = {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(d[9] == Status::Applying as u8, err(E_WRONG_PHASE));
            (
                ru64(&d, 26),
                ru64(&d, 34),
                ru32(&d, 14) as u64,
                ru64(&d, 146),
            )
        };

        check(ctx.accounts.shard.to_account_view(), DISC_SHARD, SHARD_LEN)?;
        let (sid, sbump, cursor_before, shard_epoch) = {
            let d = ctx.accounts.shard.to_account_view().try_borrow()?;
            require!(&d[16..48] == ek.as_ref(), err(E_BAD_ACCOUNT));
            let sid = ru32(&d, 12);
            require!((sid as u64) < shard_count, err(E_BAD_ACCOUNT));
            (sid, d[9], ru64(&d, 64), ru64(&d, 48))
        };
        vpda(
            ctx.accounts.shard.to_account_view(),
            &[b"cas.s", ek.as_ref(), &sid.to_le_bytes(), &[sbump]],
        )?;

        // lazy-reset prologue
        let cursor = if shard_epoch < epoch {
            let s = &mut ctx.accounts.shard;
            s.write_bytes(56, &0u64.to_le_bytes())?;
            s.write_bytes(64, &0u64.to_le_bytes())?;
            s.write_bytes(48, &epoch.to_le_bytes())?;
            0
        } else {
            cursor_before
        };

        let base = total / shard_count;
        let rem = total % shard_count;
        let len = base + if (sid as u64) < rem { 1 } else { 0 };
        let start = (sid as u64) * base + (sid as u64).min(rem);
        let n = advance_step(cursor, len, batch as u64).map_err(map_guard)?;

        // escrow binding (before reading bounty_per)
        check(
            ctx.accounts.escrow.to_account_view(),
            DISC_ESCROW,
            ESCROW_LEN,
        )?;
        let (bounty_per, xbump) = {
            let d = ctx.accounts.escrow.to_account_view().try_borrow()?;
            require!(
                &d[10..42] == ek.as_ref() && ru32(&d, 42) == sid,
                err(E_BAD_ACCOUNT)
            );
            (ru64(&d, 46), d[9])
        };
        vpda(
            ctx.accounts.escrow.to_account_view(),
            &[b"cas.e", ek.as_ref(), &sid.to_le_bytes(), &[xbump]],
        )?;
        let floor = Rent::get()?.try_minimum_balance(ESCROW_LEN)?;
        let amount = n.checked_mul(bounty_per).ok_or(err(E_BOUNTY_UNDERFUNDED))?;
        require!(
            ctx.accounts.escrow.to_account_view().lamports() >= floor + amount,
            err(E_BOUNTY_UNDERFUNDED)
        );

        {
            let ra = ctx.remaining_accounts();
            for j in 0..n {
                let gi = start + cursor + j;
                let mut m = ra.get(j as usize)?.ok_or(err(E_BAD_ACCOUNT))?;
                require!(m.owner() == &crate::ID, err(E_BAD_ACCOUNT));
                let mbump = {
                    let d = m.try_borrow_data()?;
                    require!(
                        d.len() >= MEMBER_LEN && &d[0..8] == DISC_MEMBER,
                        err(E_BAD_ACCOUNT)
                    );
                    require!(
                        &d[10..42] == ek.as_ref() && ru64(&d, 42) == gi,
                        err(E_BAD_ACCOUNT)
                    );
                    d[9]
                };
                vpda_addr(
                    m.address(),
                    &[b"cas.m", ek.as_ref(), &gi.to_le_bytes(), &[mbump]],
                )?;
                let mut d = m.try_borrow_data_mut()?;
                if ru64(&d, 58) >= epoch {
                    continue;
                }
                let cur = ru64(&d, MEMBER_BODY);
                wu64(&mut d, MEMBER_BODY, cur.saturating_add(delta));
                wu64(&mut d, 58, epoch);
            }
        }

        let escrow_view = ctx.accounts.escrow.to_account_view();
        set_lamports(escrow_view, escrow_view.lamports() - amount);
        let cranker_view = ctx.accounts.cranker.to_account_view();
        set_lamports(cranker_view, cranker_view.lamports() + amount);
        ctx.accounts
            .shard
            .write_bytes(64, &(cursor + n).to_le_bytes())?;
        Ok(())
    }

    #[instruction(discriminator = 5)]
    pub fn try_finish_apply(ctx: Ctx<TryFinish>) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        let ek = *ctx.accounts.effect.to_account_view().address();
        let (done, epoch, total, shard_count) = {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(d[9] == Status::Applying as u8, err(E_WRONG_PHASE));
            (
                ru32(&d, 18),
                ru64(&d, 26),
                ru64(&d, 34),
                ru32(&d, 14) as u64,
            )
        };
        check(ctx.accounts.shard.to_account_view(), DISC_SHARD, SHARD_LEN)?;
        let (sid, shard_epoch, shard_cursor) = {
            let d = ctx.accounts.shard.to_account_view().try_borrow()?;
            require!(&d[16..48] == ek.as_ref(), err(E_BAD_ACCOUNT));
            (ru32(&d, 12), ru64(&d, 48), ru64(&d, 64))
        };
        let base = total / shard_count;
        let rem = total % shard_count;
        let len = base + if (sid as u64) < rem { 1 } else { 0 };
        apply_finish_step(done, sid, shard_epoch, epoch, shard_cursor, len).map_err(map_guard)?;
        let next = done + 1;
        ctx.accounts.effect.write_bytes(18, &next.to_le_bytes())?;
        if next as u64 == shard_count {
            ctx.accounts.effect.write_bytes(9, &[Status::Done as u8])?;
        }
        Ok(())
    }

    #[instruction(discriminator = 6)]
    pub fn finalize(ctx: Ctx<Finalize>) -> Result<(), ProgramError> {
        check(
            ctx.accounts.effect.to_account_view(),
            DISC_EFFECT,
            EFFECT_LEN,
        )?;
        let flags = {
            let d = ctx.accounts.effect.to_account_view().try_borrow()?;
            require!(d[9] == Status::Done as u8, err(E_WRONG_PHASE));
            d[11]
        };
        ctx.accounts.effect.write_bytes(11, &[flags | (1 << 5)])?; // FINALIZED
        Ok(())
    }
}

// All accounts are raw `UncheckedAccount`s: freshet works on the byte layout, validated
// manually in the handlers (owner / discriminator / PDA). `Signer` enforces is_signer at
// parse time.
#[derive(Accounts)]
pub struct InitEffect {
    #[account(mut)]
    pub effect: UncheckedAccount,
    pub authority: Signer,
}
#[derive(Accounts)]
pub struct InitShards {
    #[account(mut)]
    pub effect: UncheckedAccount,
    #[account(mut)]
    pub shard: UncheckedAccount,
    #[account(mut)]
    pub escrow: UncheckedAccount,
    pub authority: Signer,
}
#[derive(Accounts)]
pub struct Enroll {
    #[account(mut)]
    pub effect: UncheckedAccount,
    #[account(mut)]
    pub member: UncheckedAccount,
    pub authority: Signer,
}
#[derive(Accounts)]
pub struct Seal {
    #[account(mut)]
    pub effect: UncheckedAccount,
    pub authority: Signer,
}
#[derive(Accounts)]
pub struct AdvanceApply {
    pub effect: UncheckedAccount,
    #[account(mut)]
    pub shard: UncheckedAccount,
    #[account(mut)]
    pub escrow: UncheckedAccount,
    #[account(mut)]
    pub cranker: Signer,
}
#[derive(Accounts)]
pub struct TryFinish {
    #[account(mut)]
    pub effect: UncheckedAccount,
    pub shard: UncheckedAccount,
}
#[derive(Accounts)]
pub struct Finalize {
    #[account(mut)]
    pub effect: UncheckedAccount,
}
