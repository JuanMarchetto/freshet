//! On-chain integration tests (mollusk-svm): they prove the handler wiring realizes the
//! verified core logic — a full single-pass lifecycle reaching Done with correct member
//! mutation and keeper payment, and the rejection of a cross-escrow drain.

use freshet_program::ID;
use mollusk_svm::result::ProgramResult;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

const DELTA: u64 = 5;
const BOUNTY_PER: u64 = 1_000;
const FUND: u64 = 1_000_000_000;

fn pid() -> Pubkey {
    Pubkey::new_from_array(ID)
}
fn owned_zeroed(len: usize) -> Account {
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

struct Harness {
    mollusk: Mollusk,
    ledger: HashMap<Pubkey, Account>,
}
impl Harness {
    fn new() -> Self {
        // Make the test self-contained: point mollusk at the workspace's deploy dir.
        if std::env::var("SBF_OUT_DIR").is_err() {
            std::env::set_var(
                "SBF_OUT_DIR",
                format!("{}/../target/deploy", env!("CARGO_MANIFEST_DIR")),
            );
        }
        Harness {
            mollusk: Mollusk::new(&pid(), "freshet_program"),
            ledger: HashMap::new(),
        }
    }
    fn put(&mut self, k: Pubkey, a: Account) {
        self.ledger.insert(k, a);
    }
    fn acct(&self, k: &Pubkey) -> &Account {
        self.ledger.get(k).expect("account in ledger")
    }
    /// Run an instruction, asserting success, threading resulting accounts back.
    fn ok(&mut self, metas: Vec<AccountMeta>, data: Vec<u8>) {
        let r = self.run(metas, data);
        assert!(r.is_ok(), "instruction failed: {r:?}");
    }
    fn run(&mut self, metas: Vec<AccountMeta>, data: Vec<u8>) -> ProgramResult {
        let accounts: Vec<(Pubkey, Account)> = metas
            .iter()
            .map(|m| {
                (
                    m.pubkey,
                    self.ledger.get(&m.pubkey).cloned().unwrap_or_else(wallet),
                )
            })
            .collect();
        let ix = Instruction {
            program_id: pid(),
            accounts: metas,
            data,
        };
        let res = self.mollusk.process_instruction(&ix, &accounts);
        for (k, a) in &res.resulting_accounts {
            self.ledger.insert(*k, a.clone());
        }
        res.program_result.clone()
    }
}

fn pda(seeds: &[&[u8]]) -> (Pubkey, u8) {
    Pubkey::find_program_address(seeds, &pid())
}
fn w(k: Pubkey) -> AccountMeta {
    AccountMeta::new(k, false)
}
fn ro(k: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(k, false)
}
fn signer(k: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(k, true)
}
fn signer_w(k: Pubkey) -> AccountMeta {
    AccountMeta::new(k, true)
}

fn init_effect_data(shard_count: u32, delta: u64) -> Vec<u8> {
    let mut d = vec![0u8; 1 + 1 + 1 + 4 + 128];
    d[0] = 0; // tag InitEffect
    d[1] = 0; // effect bump (effect is not a PDA in this reference)
    d[2] = 0; // flags (single-pass, push)
    d[3..7].copy_from_slice(&shard_count.to_le_bytes());
    d[7..15].copy_from_slice(&delta.to_le_bytes()); // params[0..8] = delta
    d
}

/// Full single-pass lifecycle to Done: members credited exactly once, keeper paid per item.
#[test]
fn t5_full_lifecycle_reaches_done() {
    let mut h = Harness::new();
    let effect = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let cranker = Pubkey::new_unique();
    let total: u64 = 2;

    h.put(effect, owned_zeroed(296));
    h.put(authority, wallet());
    h.put(
        cranker,
        Account {
            lamports: 0,
            ..wallet()
        },
    );

    // init_effect (shard_count=1, delta=5)
    h.ok(
        vec![w(effect), signer(authority)],
        init_effect_data(1, DELTA),
    );

    // init_shards (sid=0)
    let (shard0, sbump) = pda(&[b"cas.s", effect.as_ref(), &0u32.to_le_bytes()]);
    let (escrow0, ebump) = pda(&[b"cas.e", effect.as_ref(), &0u32.to_le_bytes()]);
    h.put(shard0, owned_zeroed(152));
    h.put(escrow0, owned_zeroed(64));
    let mut ishd = vec![1u8]; // tag InitShards
    ishd.extend_from_slice(&0u32.to_le_bytes());
    ishd.push(sbump);
    ishd.push(ebump);
    ishd.extend_from_slice(&BOUNTY_PER.to_le_bytes());
    h.ok(
        vec![w(effect), w(shard0), w(escrow0), signer(authority)],
        ishd,
    );

    // enroll members 0..total
    let mut members = vec![];
    for i in 0..total {
        let (m, mbump) = pda(&[b"cas.m", effect.as_ref(), &i.to_le_bytes()]);
        h.put(m, owned_zeroed(80)); // 72 header-pad + 8 body
        h.ok(vec![w(effect), w(m), signer(authority)], vec![2u8, mbump]);
        members.push(m);
    }

    // seal
    h.ok(vec![w(effect), signer(authority)], vec![3u8]);

    // advance_apply (batch=2): effect ro, shard w, escrow w, cranker signer-w, members w
    let mut metas = vec![ro(effect), w(shard0), w(escrow0), signer_w(cranker)];
    for m in &members {
        metas.push(w(*m));
    }
    h.ok(metas, vec![7u8, 2u8]);

    // try_finish_apply, then finalize
    h.ok(vec![w(effect), ro(shard0)], vec![8u8]);
    h.ok(vec![w(effect)], vec![9u8]);

    // ── assertions ──
    let e = h.acct(&effect);
    assert_eq!(e.data[9], 4, "status == Done(4)");
    assert_ne!(e.data[11] & (1 << 5), 0, "FINALIZED flag set");
    for m in &members {
        let body = u64::from_le_bytes(h.acct(m).data[72..80].try_into().unwrap());
        assert_eq!(body, DELTA, "member credited by delta exactly once");
    }
    assert_eq!(
        h.acct(&cranker).lamports,
        total * BOUNTY_PER,
        "keeper paid per item"
    );
}

/// A cross-escrow drain must fail: advancing effect A.s shard while passing a different
/// effect.s escrow is rejected before any payout, leaving the victim escrow untouched.
#[test]
fn t3_cross_escrow_drain_fails() {
    let mut h = Harness::new();
    let authority = Pubkey::new_unique();
    let cranker = Pubkey::new_unique();

    // Build two independent sealed single-pass effects A and B (B's escrow is fat).
    let setup = |h: &mut Harness, escrow_fund: u64| -> (Pubkey, Pubkey, Pubkey, Pubkey) {
        let effect = Pubkey::new_unique();
        h.put(effect, owned_zeroed(296));
        h.put(authority, wallet());
        h.ok(
            vec![w(effect), signer(authority)],
            init_effect_data(1, DELTA),
        );
        let (shard0, sbump) = pda(&[b"cas.s", effect.as_ref(), &0u32.to_le_bytes()]);
        let (escrow0, ebump) = pda(&[b"cas.e", effect.as_ref(), &0u32.to_le_bytes()]);
        h.put(shard0, owned_zeroed(152));
        h.put(
            escrow0,
            Account {
                lamports: escrow_fund,
                ..owned_zeroed(64)
            },
        );
        let mut ishd = vec![1u8];
        ishd.extend_from_slice(&0u32.to_le_bytes());
        ishd.push(sbump);
        ishd.push(ebump);
        ishd.extend_from_slice(&BOUNTY_PER.to_le_bytes());
        h.ok(
            vec![w(effect), w(shard0), w(escrow0), signer(authority)],
            ishd,
        );
        let (m, mbump) = pda(&[b"cas.m", effect.as_ref(), &0u64.to_le_bytes()]);
        h.put(m, owned_zeroed(80));
        h.ok(vec![w(effect), w(m), signer(authority)], vec![2u8, mbump]);
        h.ok(vec![w(effect), signer(authority)], vec![3u8]);
        (effect, shard0, escrow0, m)
    };
    let (effect_a, shard_a, _escrow_a, member_a) = setup(&mut h, FUND);
    let (_effect_b, _shard_b, escrow_b, _member_b) = setup(&mut h, FUND);
    let escrow_b_before = h.acct(&escrow_b).lamports;

    // advance_apply for A's shard but pass B's escrow → must fail the (effect,shard_id) bind.
    let metas = vec![
        ro(effect_a),
        w(shard_a),
        w(escrow_b),
        signer_w(cranker),
        w(member_a),
    ];
    let r = h.run(metas, vec![7u8, 1u8]);
    assert!(r.is_err(), "cross-escrow drain must be rejected");
    assert_eq!(
        h.acct(&escrow_b).lamports,
        escrow_b_before,
        "victim escrow B must be untouched"
    );
}
