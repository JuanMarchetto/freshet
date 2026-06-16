//! freshet on-chain program (Pinocchio) — the deployable reference settler.
//!
//! The program holds no state-machine logic of its own: each handler validates accounts
//! (SPEC §11), reads/writes the zero-copy Pod layouts (`layout`, SPEC §2), and DELEGATES
//! every transition decision to the verified pure core (`freshet::state` guards,
//! `freshet::partition`, `freshet::monoid`). See `SPEC.md`.
#![no_std]

use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, ProgramResult,
};

pinocchio::program_entrypoint!(process_instruction);
pinocchio::default_allocator!();

// Own `#[panic_handler]` (same syscalls as pinocchio's `nostd_panic_handler!`, minus the
// `#[no_mangle]` that the platform-tools rustc rejects on the panic lang item).
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

// On non-solana (host build of the `lib` target), link std for its panic handler/allocator.
#[cfg(not(target_os = "solana"))]
extern crate std;

pub mod error;
pub mod instructions;
pub mod layout;
pub mod security;

/// Program ID — placeholder until a deploy keypair is generated (fixed, non-zero so it
/// is a normal program address for tests/derivation).
pub const ID: Pubkey = [
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
];
/// Owning program of all freshet PDAs. For this standalone reference settler the program
/// IS its own consumer, so CONSUMER_ID == ID. (A library consumer would set its own.)
pub const CONSUMER_ID: Pubkey = ID;

/// 1-byte instruction tag (§6) — DISTINCT from the 8-byte account discriminator (which
/// defends type-cosplay in account *data*).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ix {
    InitEffect = 0,
    InitShards = 1,
    Enroll = 2,
    Seal = 3,
    AdvanceReduce = 4,
    ReduceShards = 5,
    BeginApply = 6,
    AdvanceApply = 7,
    TryFinishApply = 8,
    Finalize = 9,
    RefundEscrow = 10,
    TopUpBounty = 11,
    Skip = 12,
    SkipReduce = 13,
    Cancel = 14,
    Reset = 15,
}

impl TryFrom<u8> for Ix {
    type Error = ProgramError;
    fn try_from(v: u8) -> Result<Ix, ProgramError> {
        Ok(match v {
            0 => Ix::InitEffect,
            1 => Ix::InitShards,
            2 => Ix::Enroll,
            3 => Ix::Seal,
            4 => Ix::AdvanceReduce,
            5 => Ix::ReduceShards,
            6 => Ix::BeginApply,
            7 => Ix::AdvanceApply,
            8 => Ix::TryFinishApply,
            9 => Ix::Finalize,
            10 => Ix::RefundEscrow,
            11 => Ix::TopUpBounty,
            12 => Ix::Skip,
            13 => Ix::SkipReduce,
            14 => Ix::Cancel,
            15 => Ix::Reset,
            _ => return Err(ProgramError::InvalidInstructionData),
        })
    }
}

fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let (tag, data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match Ix::try_from(*tag)? {
        Ix::InitEffect => instructions::init_effect(accounts, data),
        Ix::InitShards => instructions::init_shards(accounts, data),
        Ix::Enroll => instructions::enroll(accounts, data),
        Ix::Seal => instructions::seal(accounts),
        Ix::AdvanceReduce => instructions::advance_reduce(accounts, data),
        Ix::ReduceShards => instructions::reduce_shards(accounts),
        Ix::BeginApply => instructions::begin_apply(accounts),
        Ix::AdvanceApply => instructions::advance_apply(accounts, data),
        Ix::TryFinishApply => instructions::try_finish_apply(accounts),
        Ix::Finalize => instructions::finalize(accounts),
        Ix::RefundEscrow => instructions::refund_escrow(accounts),
        Ix::TopUpBounty => instructions::top_up_bounty(accounts, data),
        Ix::Skip => instructions::skip(accounts),
        Ix::SkipReduce => instructions::skip_reduce(accounts),
        Ix::Cancel => instructions::cancel(accounts),
        Ix::Reset => instructions::reset(accounts),
    }
}

#[cfg(test)]
mod tests {
    use super::Ix;

    #[test]
    fn ix_tag_decode_is_stable_and_checked() {
        let all = [
            Ix::InitEffect,
            Ix::InitShards,
            Ix::Enroll,
            Ix::Seal,
            Ix::AdvanceReduce,
            Ix::ReduceShards,
            Ix::BeginApply,
            Ix::AdvanceApply,
            Ix::TryFinishApply,
            Ix::Finalize,
            Ix::RefundEscrow,
            Ix::TopUpBounty,
            Ix::Skip,
            Ix::SkipReduce,
            Ix::Cancel,
            Ix::Reset,
        ];
        for ix in all {
            assert_eq!(Ix::try_from(ix as u8).unwrap(), ix);
        }
        for v in 16u8..=255 {
            assert!(Ix::try_from(v).is_err(), "tag {v} must be rejected");
        }
    }
}
