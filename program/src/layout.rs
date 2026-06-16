//! Zero-copy account layouts (SPEC §2). ALL multi-byte fields are little-endian byte
//! arrays so every struct has `align_of == 1` — the sound realization of §2.1's offsets
//! (epoch@26, total@34, acc_global@82, … are ≡2 mod 8, NOT naturally aligned; a native
//! `#[repr(C)]` would repad and break every offset). Values are read/written by COPY via
//! `from_le_bytes`/`to_le_bytes` — never `&u64` references into these fields (that would
//! be an unaligned reference = UB on SBF). The `align_of == 1` const-asserts are the
//! load-bearing guard that makes the `*(ptr as *const T)` header cast sound for any
//! pointer.

use core::mem::{align_of, offset_of, size_of};
use freshet::state::{Error as CoreErr, Status};
use pinocchio::pubkey::Pubkey;

type U16 = [u8; 2];
type U32 = [u8; 4];
type U64 = [u8; 8];

// ── Discriminators (§2, anti-cosplay) + versions (§2.6) ─────────────────────────────
pub const DISC_EFFECT: [u8; 8] = *b"FRSHEFCT";
pub const DISC_SHARD: [u8; 8] = *b"FRSHSHRD";
pub const DISC_ESCROW: [u8; 8] = *b"FRSHESCR";
pub const DISC_MEMBER: [u8; 8] = *b"FRSHMMBR";

pub const VERSION_EFFECT: u8 = 2;
pub const VERSION_SHARD: u8 = 1;
pub const VERSION_ESCROW: u8 = 1;
pub const VERSION_MEMBER: u8 = 1;

// Flag bits (Effect.flags).
pub const FLAG_SHARDED: u8 = 1 << 0;
pub const FLAG_REQUIRES_REDUCE: u8 = 1 << 1;
pub const FLAG_ORDER_INDEPENDENT: u8 = 1 << 2;
pub const FLAG_PULL: u8 = 1 << 3;
pub const FLAG_ACC_EXTERNAL: u8 = 1 << 4;
pub const FLAG_FINALIZED: u8 = 1 << 5;

/// Consumer member body begins at this 8-aligned offset (6 pad bytes after the 66-byte
/// header, §2.4/§2.7).
pub const MEMBER_BODY_OFFSET: usize = 72;

// ── Effect (296 B, §2.1) ────────────────────────────────────────────────────────────
#[repr(C)]
pub struct Effect {
    pub disc: [u8; 8],
    pub version: u8,
    pub status: u8,
    pub bump: u8,
    pub flags: u8,
    pub rule_id: U16,
    pub shard_count: U32,
    pub shards_done: U32,
    pub merge_cursor: U32,
    pub epoch: U64,
    pub total: U64,
    pub reduce_skipped_count: U64,
    pub authority: Pubkey,
    pub acc_global: [u8; 64],
    pub params: [u8; 128],
    pub created_slot: U64,
    pub apply_skipped_count: U64,
    pub shards_created: U32,
    pub _reserved: [u8; 2],
}

const _: () = {
    assert!(align_of::<Effect>() == 1);
    assert!(size_of::<Effect>() == 296);
    assert!(offset_of!(Effect, shard_count) == 14);
    assert!(offset_of!(Effect, shards_done) == 18);
    assert!(offset_of!(Effect, merge_cursor) == 22);
    assert!(offset_of!(Effect, epoch) == 26);
    assert!(offset_of!(Effect, total) == 34);
    assert!(offset_of!(Effect, reduce_skipped_count) == 42);
    assert!(offset_of!(Effect, authority) == 50);
    assert!(offset_of!(Effect, acc_global) == 82);
    assert!(offset_of!(Effect, params) == 146);
    assert!(offset_of!(Effect, created_slot) == 274);
    assert!(offset_of!(Effect, apply_skipped_count) == 282);
    assert!(offset_of!(Effect, shards_created) == 290);
    assert!(offset_of!(Effect, _reserved) == 294);
};

impl Effect {
    pub const LEN: usize = 296;
    #[inline]
    pub fn pull_enabled(&self) -> bool {
        self.flags & FLAG_PULL != 0
    }
    #[inline]
    pub fn requires_reduce(&self) -> bool {
        self.flags & FLAG_REQUIRES_REDUCE != 0
    }
    #[inline]
    pub fn status(&self) -> Result<Status, CoreErr> {
        Status::try_from(self.status) // checked; ≥6 ⇒ BadStatus (§2.1)
    }
    #[inline]
    pub fn set_status(&mut self, s: Status) {
        self.status = s as u8;
    }
    #[inline]
    pub fn epoch(&self) -> u64 {
        u64::from_le_bytes(self.epoch)
    }
    #[inline]
    pub fn set_epoch(&mut self, v: u64) {
        self.epoch = v.to_le_bytes();
    }
    #[inline]
    pub fn total(&self) -> u64 {
        u64::from_le_bytes(self.total)
    }
    #[inline]
    pub fn shard_count(&self) -> u32 {
        u32::from_le_bytes(self.shard_count)
    }
    #[inline]
    pub fn shards_done(&self) -> u32 {
        u32::from_le_bytes(self.shards_done)
    }
    #[inline]
    pub fn set_shards_done(&mut self, v: u32) {
        self.shards_done = v.to_le_bytes();
    }
    #[inline]
    pub fn merge_cursor(&self) -> u32 {
        u32::from_le_bytes(self.merge_cursor)
    }
    #[inline]
    pub fn set_merge_cursor(&mut self, v: u32) {
        self.merge_cursor = v.to_le_bytes();
    }
    #[inline]
    pub fn shards_created(&self) -> u32 {
        u32::from_le_bytes(self.shards_created)
    }
}

// ── Shard (152 B, §2.2) ─────────────────────────────────────────────────────────────
#[repr(C)]
pub struct Shard {
    pub disc: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub _pad: [u8; 2],
    pub shard_id: U32,
    pub effect: Pubkey,
    pub epoch: U64,
    pub reduce_cursor: U64,
    pub apply_cursor: U64,
    pub acc_partial: [u8; 64],
    pub _reserved: [u8; 16],
}

const _: () = {
    assert!(align_of::<Shard>() == 1);
    assert!(size_of::<Shard>() == 152);
    assert!(offset_of!(Shard, shard_id) == 12);
    assert!(offset_of!(Shard, effect) == 16);
    assert!(offset_of!(Shard, epoch) == 48);
    assert!(offset_of!(Shard, reduce_cursor) == 56);
    assert!(offset_of!(Shard, apply_cursor) == 64);
    assert!(offset_of!(Shard, acc_partial) == 72);
    assert!(offset_of!(Shard, _reserved) == 136);
};

impl Shard {
    pub const LEN: usize = 152;
    #[inline]
    pub fn shard_id(&self) -> u32 {
        u32::from_le_bytes(self.shard_id)
    }
    #[inline]
    pub fn epoch(&self) -> u64 {
        u64::from_le_bytes(self.epoch)
    }
    #[inline]
    pub fn set_epoch(&mut self, v: u64) {
        self.epoch = v.to_le_bytes();
    }
    #[inline]
    pub fn reduce_cursor(&self) -> u64 {
        u64::from_le_bytes(self.reduce_cursor)
    }
    #[inline]
    pub fn set_reduce_cursor(&mut self, v: u64) {
        self.reduce_cursor = v.to_le_bytes();
    }
    #[inline]
    pub fn apply_cursor(&self) -> u64 {
        u64::from_le_bytes(self.apply_cursor)
    }
    #[inline]
    pub fn set_apply_cursor(&mut self, v: u64) {
        self.apply_cursor = v.to_le_bytes();
    }
    /// Universal lazy per-epoch reset (SPEC §5): a shard left over from a prior epoch is
    /// rewound on its first touch this epoch. Run by every shard-mutating handler before
    /// reading the cursor, so a `skip` as the first touch can't advance a stale cursor.
    #[inline]
    pub fn lazy_reset(&mut self, epoch: u64) {
        if self.epoch() < epoch {
            self.set_reduce_cursor(0);
            self.set_apply_cursor(0);
            self.acc_partial = [0u8; 64];
            self.set_epoch(epoch);
        }
    }
}

// ── Escrow (64 B, §2.3) ─────────────────────────────────────────────────────────────
#[repr(C)]
pub struct Escrow {
    pub disc: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub effect: Pubkey,
    pub shard_id: U32,
    pub bounty_per: U64,
    pub last_refund_epoch: U64,
    pub _reserved: [u8; 2],
}

const _: () = {
    assert!(align_of::<Escrow>() == 1);
    assert!(size_of::<Escrow>() == 64);
    assert!(offset_of!(Escrow, effect) == 10);
    assert!(offset_of!(Escrow, shard_id) == 42);
    assert!(offset_of!(Escrow, bounty_per) == 46);
    assert!(offset_of!(Escrow, last_refund_epoch) == 54);
    assert!(offset_of!(Escrow, _reserved) == 62);
};

impl Escrow {
    pub const LEN: usize = 64;
    #[inline]
    pub fn shard_id(&self) -> u32 {
        u32::from_le_bytes(self.shard_id)
    }
    #[inline]
    pub fn bounty_per(&self) -> u64 {
        u64::from_le_bytes(self.bounty_per)
    }
    #[inline]
    pub fn last_refund_epoch(&self) -> u64 {
        u64::from_le_bytes(self.last_refund_epoch)
    }
    #[inline]
    pub fn set_last_refund_epoch(&mut self, v: u64) {
        self.last_refund_epoch = v.to_le_bytes();
    }
}

// ── Member header (66 B, §2.4); consumer body at MEMBER_BODY_OFFSET ─────────────────
#[repr(C)]
pub struct MemberHeader {
    pub disc: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub effect: Pubkey,
    pub index: U64,
    pub last_reduce_epoch: U64,
    pub last_apply_epoch: U64,
}

const _: () = {
    assert!(align_of::<MemberHeader>() == 1);
    assert!(size_of::<MemberHeader>() == 66);
    assert!(offset_of!(MemberHeader, effect) == 10);
    assert!(offset_of!(MemberHeader, index) == 42);
    assert!(offset_of!(MemberHeader, last_reduce_epoch) == 50);
    assert!(offset_of!(MemberHeader, last_apply_epoch) == 58);
    assert!(MEMBER_BODY_OFFSET >= 66);
};

impl MemberHeader {
    pub const LEN: usize = 66;
    #[inline]
    pub fn index(&self) -> u64 {
        u64::from_le_bytes(self.index)
    }
    #[inline]
    pub fn last_reduce_epoch(&self) -> u64 {
        u64::from_le_bytes(self.last_reduce_epoch)
    }
    #[inline]
    pub fn set_last_reduce_epoch(&mut self, v: u64) {
        self.last_reduce_epoch = v.to_le_bytes();
    }
    #[inline]
    pub fn last_apply_epoch(&self) -> u64 {
        u64::from_le_bytes(self.last_apply_epoch)
    }
    #[inline]
    pub fn set_last_apply_epoch(&mut self, v: u64) {
        self.last_apply_epoch = v.to_le_bytes();
    }
}
