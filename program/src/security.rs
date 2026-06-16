//! SPEC §11 security helpers. Validation runs on the already-borrowed account bytes; the
//! handler holds the `RefMut<[u8]>` and casts to `&mut Layout` (sound because every
//! layout is `align_of == 1`, asserted in `layout`, and the length is checked here).

use crate::error::{
    custom, E_ACCOUNT_DATA_TOO_SMALL, E_BAD_BUMP, E_BAD_DISCRIMINATOR, E_BAD_OWNER, E_BAD_VERSION,
    E_BOUNTY_UNDERFUNDED, E_UNAUTHORIZED,
};
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::{create_program_address, Pubkey},
    sysvars::{rent::Rent, Sysvar},
};

/// §11: owner == consumer, length ≥ min, discriminator matches, version supported. Run
/// on the borrowed slice BEFORE casting it to a layout struct.
pub fn check_header(
    ai: &AccountInfo,
    consumer: &Pubkey,
    data: &[u8],
    disc: &[u8; 8],
    version: u8,
    min_len: usize,
) -> Result<(), ProgramError> {
    if !ai.is_owned_by(consumer) {
        return Err(custom(E_BAD_OWNER));
    }
    if data.len() < min_len {
        return Err(custom(E_ACCOUNT_DATA_TOO_SMALL));
    }
    if &data[0..8] != disc {
        return Err(custom(E_BAD_DISCRIMINATOR));
    }
    if data[8] > version {
        return Err(custom(E_BAD_VERSION));
    }
    Ok(())
}

/// §3: the account key must equal the canonical PDA for `seeds` (caller appends `&[bump]`).
pub fn verify_pda(key: &Pubkey, seeds: &[&[u8]], consumer: &Pubkey) -> Result<(), ProgramError> {
    let derived = create_program_address(seeds, consumer).map_err(|_| custom(E_BAD_BUMP))?;
    if &derived != key {
        return Err(custom(E_BAD_BUMP));
    }
    Ok(())
}

/// §6 authorizer (W/P): the stored authority must be the account, and it must sign. An
/// `invoke_signed` PDA authority arrives as a signer, so this covers the P path too.
pub fn authorize(authority_ai: &AccountInfo, stored: &Pubkey) -> Result<(), ProgramError> {
    if authority_ai.key() != stored || !authority_ai.is_signer() {
        return Err(custom(E_UNAUTHORIZED));
    }
    Ok(())
}

/// Rent-exempt minimum for an account of `len` bytes (§8 escrow floor; §2.3 funding).
pub fn rent_floor(len: usize) -> Result<u64, ProgramError> {
    Ok(Rent::get()?.minimum_balance(len))
}

/// §8 all-or-nothing bounty precheck. `n` is on-chain-derived (never caller-supplied).
/// Returns the payable amount, or `BountyUnderfunded` if the escrow can't cover it above
/// its rent floor.
pub fn pay_amount(
    escrow_lamports: u64,
    floor: u64,
    n: u64,
    bounty_per: u64,
) -> Result<u64, ProgramError> {
    let amount = n
        .checked_mul(bounty_per)
        .ok_or(custom(E_BOUNTY_UNDERFUNDED))?;
    let spendable = escrow_lamports
        .checked_sub(floor)
        .ok_or(custom(E_BOUNTY_UNDERFUNDED))?;
    if spendable < amount {
        return Err(custom(E_BOUNTY_UNDERFUNDED));
    }
    Ok(amount)
}

/// Direct lamport swap between two consumer-owned accounts (§2.3 makes a System CPI
/// unnecessary; SPEC §6.1 erratum blesses the direct move).
pub fn transfer_lamports(
    from: &AccountInfo,
    to: &AccountInfo,
    amount: u64,
) -> Result<(), ProgramError> {
    if !from.is_writable() || !to.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut f = from.try_borrow_mut_lamports()?;
    let mut t = to.try_borrow_mut_lamports()?;
    *f = f
        .checked_sub(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    *t = t
        .checked_add(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    Ok(())
}
