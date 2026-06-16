//! Compute-unit measurements for the Pinocchio reference program (mollusk). Run with:
//!   SBF_OUT_DIR=target/deploy cargo test -p freshet-program --test bench_cu -- --nocapture
//! Reports per-instruction CU and an advance_apply overhead/per-item split. Numbers feed
//! `BENCHMARK.md`; they are the freshet *overhead* (the demo `apply` is a trivial add, so
//! per-item cost is dominated by per-member PDA derivation + header checks, not consumer
//! logic — a real consumer adds its own `apply` cost on top).

use freshet_program::ID;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

const BOUNTY_PER: u64 = 1_000;
const FUND: u64 = 1_000_000_000;

fn pid() -> Pubkey {
    Pubkey::new_from_array(ID)
}
fn owned(len: usize) -> Account {
    Account {
        lamports: FUND,
        data: vec![0u8; len],
        owner: pid(),
        executable: false,
        rent_epoch: 0,
    }
}
fn wallet() -> Account {
    Account {
        lamports: FUND,
        data: vec![],
        owner: Pubkey::default(),
        executable: false,
        rent_epoch: 0,
    }
}
fn w(k: Pubkey) -> AccountMeta {
    AccountMeta::new(k, false)
}
fn ro(k: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(k, false)
}
fn sgn(k: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(k, true)
}
fn sgn_w(k: Pubkey) -> AccountMeta {
    AccountMeta::new(k, true)
}
fn pda(seeds: &[&[u8]]) -> (Pubkey, u8) {
    Pubkey::find_program_address(seeds, &pid())
}

struct H {
    m: Mollusk,
    led: HashMap<Pubkey, Account>,
}
impl H {
    fn new() -> Self {
        if std::env::var("SBF_OUT_DIR").is_err() {
            std::env::set_var(
                "SBF_OUT_DIR",
                format!("{}/../target/deploy", env!("CARGO_MANIFEST_DIR")),
            );
        }
        H {
            m: Mollusk::new(&pid(), "freshet_program"),
            led: HashMap::new(),
        }
    }
    /// Run, thread accounts, return (ok, compute_units).
    fn go(&mut self, metas: Vec<AccountMeta>, data: Vec<u8>) -> (bool, u64) {
        let accts: Vec<(Pubkey, Account)> = metas
            .iter()
            .map(|m| {
                (
                    m.pubkey,
                    self.led.get(&m.pubkey).cloned().unwrap_or_else(wallet),
                )
            })
            .collect();
        let ix = Instruction {
            program_id: pid(),
            accounts: metas,
            data,
        };
        let r = self.m.process_instruction(&ix, &accts);
        for (k, a) in &r.resulting_accounts {
            self.led.insert(*k, a.clone());
        }
        (r.program_result.is_ok(), r.compute_units_consumed)
    }
}

fn init_effect_data(shard_count: u32) -> Vec<u8> {
    let mut d = vec![0u8; 1 + 1 + 1 + 4 + 128];
    d[3..7].copy_from_slice(&shard_count.to_le_bytes());
    d[7..15].copy_from_slice(&5u64.to_le_bytes());
    d
}

/// Build a sealed single-pass effect with `total` members in one shard; returns the
/// member pubkeys + the shard/escrow keys, plus the setup CU for the first of each ix.
fn sealed(h: &mut H, total: u64) -> (Pubkey, Pubkey, Pubkey, Vec<Pubkey>) {
    let effect = Pubkey::new_unique();
    let auth = Pubkey::new_unique();
    h.led.insert(effect, owned(296));
    h.led.insert(auth, wallet());
    assert!(h.go(vec![w(effect), sgn(auth)], init_effect_data(1)).0);
    let (shard, sb) = pda(&[b"cas.s", effect.as_ref(), &0u32.to_le_bytes()]);
    let (escrow, eb) = pda(&[b"cas.e", effect.as_ref(), &0u32.to_le_bytes()]);
    h.led.insert(shard, owned(152));
    h.led.insert(escrow, owned(64));
    let mut d = vec![1u8];
    d.extend_from_slice(&0u32.to_le_bytes());
    d.push(sb);
    d.push(eb);
    d.extend_from_slice(&BOUNTY_PER.to_le_bytes());
    assert!(h.go(vec![w(effect), w(shard), w(escrow), sgn(auth)], d).0);
    let mut members = vec![];
    for i in 0..total {
        let (mk, mb) = pda(&[b"cas.m", effect.as_ref(), &i.to_le_bytes()]);
        h.led.insert(mk, owned(80));
        assert!(h.go(vec![w(effect), w(mk), sgn(auth)], vec![2u8, mb]).0);
        members.push(mk);
    }
    assert!(h.go(vec![w(effect), sgn(auth)], vec![3u8]).0);
    (effect, shard, escrow, members)
}

/// Measure one `advance_apply` over `batch` members on a freshly-sealed effect.
fn advance_cu(batch: u64) -> u64 {
    let mut h = H::new();
    let (effect, shard, escrow, members) = sealed(&mut h, batch);
    let cranker = Pubkey::new_unique();
    h.led.insert(
        cranker,
        Account {
            lamports: 0,
            ..wallet()
        },
    );
    let mut metas = vec![ro(effect), w(shard), w(escrow), sgn_w(cranker)];
    for m in &members {
        metas.push(w(*m));
    }
    let (ok, cu) = h.go(metas, vec![7u8, batch as u8]);
    assert!(ok, "advance_apply batch={batch} failed");
    cu
}

#[test]
fn report_compute_units() {
    println!("\n=== freshet (Pinocchio) compute units — mollusk ===");

    // Lifecycle ops (first-call CU on a 2-member, 1-shard effect).
    {
        let mut h = H::new();
        let effect = Pubkey::new_unique();
        let auth = Pubkey::new_unique();
        h.led.insert(effect, owned(296));
        h.led.insert(auth, wallet());
        let (_, cu_init) = h.go(vec![w(effect), sgn(auth)], init_effect_data(1));
        println!("init_effect            : {cu_init} CU");
        let (shard, sb) = pda(&[b"cas.s", effect.as_ref(), &0u32.to_le_bytes()]);
        let (escrow, eb) = pda(&[b"cas.e", effect.as_ref(), &0u32.to_le_bytes()]);
        h.led.insert(shard, owned(152));
        h.led.insert(escrow, owned(64));
        let mut d = vec![1u8];
        d.extend_from_slice(&0u32.to_le_bytes());
        d.push(sb);
        d.push(eb);
        d.extend_from_slice(&BOUNTY_PER.to_le_bytes());
        let (_, cu_shards) = h.go(vec![w(effect), w(shard), w(escrow), sgn(auth)], d);
        println!("init_shards            : {cu_shards} CU");
        let (mk, mb) = pda(&[b"cas.m", effect.as_ref(), &0u64.to_le_bytes()]);
        h.led.insert(mk, owned(80));
        let (mk2, mb2) = pda(&[b"cas.m", effect.as_ref(), &1u64.to_le_bytes()]);
        h.led.insert(mk2, owned(80));
        let (_, cu_enroll) = h.go(vec![w(effect), w(mk), sgn(auth)], vec![2u8, mb]);
        h.go(vec![w(effect), w(mk2), sgn(auth)], vec![2u8, mb2]);
        println!("enroll (1 member)      : {cu_enroll} CU");
        let (_, cu_seal) = h.go(vec![w(effect), sgn(auth)], vec![3u8]);
        println!("seal                   : {cu_seal} CU");
        let cranker = Pubkey::new_unique();
        h.led.insert(
            cranker,
            Account {
                lamports: 0,
                ..wallet()
            },
        );
        let (_, cu_fin) = {
            // advance both then finish+finalize for their CU
            let metas = vec![
                ro(effect),
                w(shard),
                w(escrow),
                sgn_w(cranker),
                w(mk),
                w(mk2),
            ];
            h.go(metas, vec![7u8, 2u8])
        };
        println!("advance_apply (batch=2): {cu_fin} CU");
        let (_, cu_tfa) = h.go(vec![w(effect), ro(shard)], vec![8u8]);
        println!("try_finish_apply       : {cu_tfa} CU");
        let (_, cu_finalize) = h.go(vec![w(effect)], vec![9u8]);
        println!("finalize               : {cu_finalize} CU");
    }

    // advance_apply CU vs batch → overhead (intercept) + per-item (slope).
    println!("\n-- advance_apply scaling (single shard) --");
    let mut pts = vec![];
    for b in [1u64, 2, 4, 8] {
        let cu = advance_cu(b);
        println!("  batch={b:<2}            : {cu} CU");
        pts.push((b as f64, cu as f64));
    }
    // least-squares slope/intercept
    let n = pts.len() as f64;
    let sx: f64 = pts.iter().map(|p| p.0).sum();
    let sy: f64 = pts.iter().map(|p| p.1).sum();
    let sxx: f64 = pts.iter().map(|p| p.0 * p.0).sum();
    let sxy: f64 = pts.iter().map(|p| p.0 * p.1).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let intercept = (sy - slope * sx) / n;
    println!("\n  fit: overhead ≈ {intercept:.0} CU + {slope:.0} CU/member");
    println!("=== end ===\n");
}
