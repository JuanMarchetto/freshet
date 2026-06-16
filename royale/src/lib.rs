//! freshet-royale — a battle-royale settlement built on the freshet core.
//!
//! Each round, every enrolled player must `act`. A permissionless `sweep` then walks the
//! roster and eliminates everyone who did NOT act this round — *including players who are
//! offline and never showed up*. That "the effect happens whether or not the owner
//! appears" is exactly the push-mode case a pull-based (claim-when-you-touch-it) design
//! cannot express, and it is why freshet exists.
//!
//! The on-chain control flow (phase, cursor, exactly-once) is delegated to the verified
//! `freshet::state` guards; this crate only adds the Round/Player layouts, the elimination
//! `apply`, and account validation. (A production version would layer freshet's sharding +
//! keeper-bounty escrow; this demo keeps a single implicit shard and an unpaid
//! permissionless sweep to focus on the mechanic.)
#![no_std]

use freshet::state::{advance_step, Status};
use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, ProgramResult,
};

pinocchio::program_entrypoint!(process_instruction);
pinocchio::default_allocator!();

#[cfg(target_os = "solana")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    if let Some(loc) = info.location() {
        unsafe {
            pinocchio::syscalls::sol_panic_(
                loc.file().as_ptr(),
                loc.file().len() as u64,
                loc.line() as u64,
                loc.column() as u64,
            )
        }
    } else {
        unsafe { pinocchio::syscalls::abort() }
    }
}
#[cfg(not(target_os = "solana"))]
extern crate std;

pub const ID: Pubkey = [
    9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
];

const DISC_ROUND: [u8; 8] = *b"FRSHROND";
const DISC_PLAYER: [u8; 8] = *b"FRSHPLYR";

// Custom error codes.
const E_WRONG_PHASE: u32 = 0;
const E_SHARD_COMPLETE: u32 = 1;
const E_BAD: u32 = 7; // owner/disc/derivation/bounds
const E_UNAUTH: u32 = 11;
const E_DEAD: u32 = 30;

fn err(c: u32) -> ProgramError {
    ProgramError::Custom(c)
}
fn rd64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[0..8].try_into().unwrap())
}

// ── Round control account (single implicit shard; cursor over [0, total)) ───────────
// 0 disc[8] · 8 ver · 9 status · 10 bump · 11 _pad · 12 round u64 · 20 total u64 ·
// 28 cursor u64 · 36 authority[32]  → 68 bytes
const ROUND_LEN: usize = 68;
// ── Player member: 0 disc[8] · 8 ver · 9 bump · 10 _pad[2] · 12 round_key[32] ·
// 44 index u64 · 52 owner[32] · 84 acted_round u64 · 92 alive u8  → 93 bytes
const PLAYER_LEN: usize = 93;

fn round_status(d: &[u8]) -> Result<Status, ProgramError> {
    Status::try_from(d[9]).map_err(|_| err(E_WRONG_PHASE))
}

fn check(ai: &AccountInfo, disc: &[u8; 8], len: usize) -> Result<(), ProgramError> {
    if !ai.is_owned_by(&ID) {
        return Err(err(E_BAD));
    }
    let d = ai.try_borrow_data()?;
    if d.len() < len || d[0..8] != *disc {
        return Err(err(E_BAD));
    }
    Ok(())
}

fn verify_pda(key: &Pubkey, seeds: &[&[u8]]) -> Result<(), ProgramError> {
    let derived = pinocchio::pubkey::create_program_address(seeds, &ID).map_err(|_| err(E_BAD))?;
    if &derived != key {
        return Err(err(E_BAD));
    }
    Ok(())
}

fn process_instruction(_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let (tag, rest) = data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match tag {
        0 => open_round(accounts, rest),
        1 => join(accounts, rest),
        2 => lock(accounts),
        3 => act(accounts),
        4 => sweep(accounts, rest),
        5 => finish(accounts),
        6 => next_round(accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

/// open_round: data=[bump]. Accounts:[round(w, pre-alloc 68 zeroed), authority(signer)].
fn open_round(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [round, authority, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.is_empty() || !authority.is_signer() {
        return Err(err(E_UNAUTH));
    }
    if !round.is_owned_by(&ID) {
        return Err(err(E_BAD));
    }
    let mut d = round.try_borrow_mut_data()?;
    if d.len() < ROUND_LEN || d[0..8] != [0u8; 8] {
        return Err(err(E_BAD));
    }
    d[0..8].copy_from_slice(&DISC_ROUND);
    d[8] = 1; // version
    d[9] = Status::Pending as u8;
    d[10] = data[0]; // bump
    d[12..20].copy_from_slice(&1u64.to_le_bytes()); // round = 1 (0 = never-acted sentinel)
    d[20..28].copy_from_slice(&0u64.to_le_bytes()); // total
    d[28..36].copy_from_slice(&0u64.to_le_bytes()); // cursor
    d[36..68].copy_from_slice(authority.key());
    Ok(())
}

/// join: data=[player_bump, owner(32)]. Accounts:[round(w), player(w, pre-alloc), authority(signer)].
fn join(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [round, player, authority, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if data.len() < 33 {
        return Err(ProgramError::InvalidInstructionData);
    }
    check(round, &DISC_ROUND, ROUND_LEN)?;
    let mut rd = round.try_borrow_mut_data()?;
    if rd[36..68] != *authority.key() || !authority.is_signer() {
        return Err(err(E_UNAUTH));
    }
    if round_status(&rd)? != Status::Pending {
        return Err(err(E_WRONG_PHASE));
    }
    let index = rd64(&rd[20..28]);
    verify_pda(
        player.key(),
        &[b"ply", round.key(), &index.to_le_bytes(), &[data[0]]],
    )?;
    if !player.is_owned_by(&ID) {
        return Err(err(E_BAD));
    }
    let mut pd = player.try_borrow_mut_data()?;
    if pd.len() < PLAYER_LEN || pd[0..8] != [0u8; 8] {
        return Err(err(E_BAD));
    }
    pd[0..8].copy_from_slice(&DISC_PLAYER);
    pd[8] = 1;
    pd[9] = data[0]; // bump
    pd[12..44].copy_from_slice(round.key());
    pd[44..52].copy_from_slice(&index.to_le_bytes());
    pd[52..84].copy_from_slice(&data[1..33]); // owner
    pd[84..92].copy_from_slice(&0u64.to_le_bytes()); // acted_round = 0
    pd[92] = 1; // alive
    rd[20..28].copy_from_slice(&(index + 1).to_le_bytes());
    Ok(())
}

/// lock: start the round's action window. Accounts:[round(w), authority(signer)].
fn lock(accounts: &[AccountInfo]) -> ProgramResult {
    let [round, authority, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check(round, &DISC_ROUND, ROUND_LEN)?;
    let mut rd = round.try_borrow_mut_data()?;
    if rd[36..68] != *authority.key() || !authority.is_signer() {
        return Err(err(E_UNAUTH));
    }
    if round_status(&rd)? != Status::Pending || rd64(&rd[20..28]) == 0 {
        return Err(err(E_WRONG_PHASE));
    }
    rd[28..36].copy_from_slice(&0u64.to_le_bytes()); // cursor
    rd[9] = Status::Applying as u8;
    Ok(())
}

/// act: a player proves liveness this round. Accounts:[round(ro), player(w), owner(signer)].
fn act(accounts: &[AccountInfo]) -> ProgramResult {
    let [round, player, owner, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check(round, &DISC_ROUND, ROUND_LEN)?;
    check(player, &DISC_PLAYER, PLAYER_LEN)?;
    let rd = round.try_borrow_data()?;
    if round_status(&rd)? != Status::Applying {
        return Err(err(E_WRONG_PHASE));
    }
    let cur_round = rd64(&rd[12..20]);
    let mut pd = player.try_borrow_mut_data()?;
    if pd[12..44] != *round.key() {
        return Err(err(E_BAD));
    }
    if pd[52..84] != *owner.key() || !owner.is_signer() {
        return Err(err(E_UNAUTH));
    }
    if pd[92] == 0 {
        return Err(err(E_DEAD)); // eliminated players can't act
    }
    pd[84..92].copy_from_slice(&cur_round.to_le_bytes());
    Ok(())
}

/// sweep: data=[batch]. Accounts:[round(w), player_cursor..]. Eliminates every player in
/// the batch who did not act this round — including offline ones. This is the push-mode
/// effect; cursor/`n` come from the verified `advance_step`.
fn sweep(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [round, players @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let batch = *data.first().ok_or(ProgramError::InvalidInstructionData)? as u64;
    check(round, &DISC_ROUND, ROUND_LEN)?;
    let mut rd = round.try_borrow_mut_data()?;
    if round_status(&rd)? != Status::Applying {
        return Err(err(E_WRONG_PHASE));
    }
    let cur_round = rd64(&rd[12..20]);
    let total = rd64(&rd[20..28]);
    let cursor = rd64(&rd[28..36]);
    let n = advance_step(cursor, total, batch).map_err(|_| err(E_SHARD_COMPLETE))?;
    if (players.len() as u64) < n {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    for j in 0..n {
        let gi = cursor + j;
        let p = &players[j as usize];
        check(p, &DISC_PLAYER, PLAYER_LEN)?;
        let mut pd = p.try_borrow_mut_data()?;
        if pd[12..44] != *round.key() || rd64(&pd[44..52]) != gi {
            return Err(err(E_BAD));
        }
        verify_pda(p.key(), &[b"ply", round.key(), &gi.to_le_bytes(), &[pd[9]]])?;
        // Eliminate anyone who didn't act this round (offline players included).
        if rd64(&pd[84..92]) != cur_round {
            pd[92] = 0;
        }
    }
    rd[28..36].copy_from_slice(&(cursor + n).to_le_bytes());
    Ok(())
}

/// finish: the round is resolved once the cursor covers the roster. Accounts:[round(w)].
fn finish(accounts: &[AccountInfo]) -> ProgramResult {
    let [round, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check(round, &DISC_ROUND, ROUND_LEN)?;
    let mut rd = round.try_borrow_mut_data()?;
    if round_status(&rd)? != Status::Applying {
        return Err(err(E_WRONG_PHASE));
    }
    if rd64(&rd[28..36]) != rd64(&rd[20..28]) {
        return Err(err(E_SHARD_COMPLETE)); // not all players swept yet
    }
    rd[9] = Status::Done as u8;
    Ok(())
}

/// next_round: open the next action window for survivors. Accounts:[round(w), authority(signer)].
fn next_round(accounts: &[AccountInfo]) -> ProgramResult {
    let [round, authority, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    check(round, &DISC_ROUND, ROUND_LEN)?;
    let mut rd = round.try_borrow_mut_data()?;
    if rd[36..68] != *authority.key() || !authority.is_signer() {
        return Err(err(E_UNAUTH));
    }
    if round_status(&rd)? != Status::Done {
        return Err(err(E_WRONG_PHASE));
    }
    let next = rd64(&rd[12..20]) + 1;
    rd[12..20].copy_from_slice(&next.to_le_bytes());
    rd[28..36].copy_from_slice(&0u64.to_le_bytes());
    rd[9] = Status::Applying as u8;
    Ok(())
}
