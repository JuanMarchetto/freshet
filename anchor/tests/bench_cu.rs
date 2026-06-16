//! Compute-unit measurements for the Anchor port (mollusk), mirroring
//! `program/tests/bench_cu.rs` so the two are apples-to-apples. Anchor instruction data
//! is the 8-byte `global:<method>` sighash + borsh args (built here without depending on
//! anchor-lang, to avoid mixing solana crate majors). Run with:
//!   SBF_OUT_DIR=target/deploy cargo test -p freshet-anchor --test bench_cu -- --nocapture

use mollusk_svm::Mollusk;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

// freshet_anchor::ID = DtLXaBhEudsViGhmpRu8K1WrudJp2QCMCS98H7tzQF5y
const PROGRAM_ID: Pubkey = solana_pubkey::pubkey!("DtLXaBhEudsViGhmpRu8K1WrudJp2QCMCS98H7tzQF5y");
const BOUNTY_PER: u64 = 1_000;
const FUND: u64 = 1_000_000_000;

fn disc(method: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{method}").as_bytes());
    h[0..8].try_into().unwrap()
}
fn owned(len: usize) -> Account {
    Account {
        lamports: FUND,
        data: vec![0u8; len],
        owner: PROGRAM_ID,
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
    Pubkey::find_program_address(seeds, &PROGRAM_ID)
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
            m: Mollusk::new(&PROGRAM_ID, "freshet_anchor"),
            led: HashMap::new(),
        }
    }
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
            program_id: PROGRAM_ID,
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

fn init_effect_data(shard_count: u32, delta: u64) -> Vec<u8> {
    let mut d = disc("init_effect").to_vec();
    d.push(0); // flags
    d.extend_from_slice(&shard_count.to_le_bytes());
    d.extend_from_slice(&delta.to_le_bytes());
    d
}

fn sealed(h: &mut H, total: u64) -> (Pubkey, Pubkey, Pubkey, Vec<Pubkey>) {
    let effect = Pubkey::new_unique();
    let auth = Pubkey::new_unique();
    h.led.insert(effect, owned(296));
    h.led.insert(auth, wallet());
    assert!(
        h.go(vec![w(effect), sgn(auth)], init_effect_data(1, 5)).0,
        "init_effect"
    );
    let (shard, sb) = pda(&[b"cas.s", effect.as_ref(), &0u32.to_le_bytes()]);
    let (escrow, eb) = pda(&[b"cas.e", effect.as_ref(), &0u32.to_le_bytes()]);
    h.led.insert(shard, owned(152));
    h.led.insert(escrow, owned(64));
    let mut d = disc("init_shards").to_vec();
    d.extend_from_slice(&0u32.to_le_bytes());
    d.push(sb);
    d.push(eb);
    d.extend_from_slice(&BOUNTY_PER.to_le_bytes());
    assert!(
        h.go(vec![w(effect), w(shard), w(escrow), sgn(auth)], d).0,
        "init_shards"
    );
    let mut members = vec![];
    for i in 0..total {
        let (mk, mb) = pda(&[b"cas.m", effect.as_ref(), &i.to_le_bytes()]);
        h.led.insert(mk, owned(80));
        let mut d = disc("enroll").to_vec();
        d.push(mb);
        assert!(h.go(vec![w(effect), w(mk), sgn(auth)], d).0, "enroll");
        members.push(mk);
    }
    assert!(
        h.go(vec![w(effect), sgn(auth)], disc("seal").to_vec()).0,
        "seal"
    );
    (effect, shard, escrow, members)
}

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
    let mut d = disc("advance_apply").to_vec();
    d.push(batch as u8);
    let (ok, cu) = h.go(metas, d);
    assert!(ok, "advance_apply batch={batch}");
    cu
}

#[test]
fn report_compute_units() {
    println!("\n=== freshet (Anchor) compute units — mollusk ===");
    let mut h = H::new();
    let (effect, shard, escrow, members) = sealed(&mut h, 2);
    let cranker = Pubkey::new_unique();
    h.led.insert(
        cranker,
        Account {
            lamports: 0,
            ..wallet()
        },
    );
    let metas = vec![
        ro(effect),
        w(shard),
        w(escrow),
        sgn_w(cranker),
        w(members[0]),
        w(members[1]),
    ];
    let mut d = disc("advance_apply").to_vec();
    d.push(2u8);
    let (_, cu_adv) = h.go(metas, d);
    println!("advance_apply (batch=2): {cu_adv} CU");
    let (_, cu_tfa) = h.go(
        vec![w(effect), ro(shard)],
        disc("try_finish_apply").to_vec(),
    );
    println!("try_finish_apply       : {cu_tfa} CU");
    let (_, cu_fin) = h.go(vec![w(effect)], disc("finalize").to_vec());
    println!("finalize               : {cu_fin} CU");

    println!("\n-- advance_apply scaling (single shard) --");
    let mut pts = vec![];
    for b in [1u64, 2, 4, 8] {
        let cu = advance_cu(b);
        println!("  batch={b:<2}            : {cu} CU");
        pts.push((b as f64, cu as f64));
    }
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
