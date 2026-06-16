//! The headline freshet use case, on-chain: a round eliminates the player who never
//! acted — even though that player's owner never sent a transaction. A pull-based design
//! can't do this (there's no one to "claim" their own elimination); freshet's push-mode
//! crank can. Proven end-to-end on a real SVM via mollusk.

use freshet_royale::ID;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

const FUND: u64 = 1_000_000_000;
const ALIVE_OFFSET: usize = 92;
const STATUS_OFFSET: usize = 9;

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
            m: Mollusk::new(&pid(), "freshet_royale"),
            led: HashMap::new(),
        }
    }
    fn put(&mut self, k: Pubkey, a: Account) {
        self.led.insert(k, a);
    }
    fn ok(&mut self, metas: Vec<AccountMeta>, data: Vec<u8>) {
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
        assert!(
            r.program_result.is_ok(),
            "instruction failed: {:?}",
            r.program_result
        );
        for (k, a) in &r.resulting_accounts {
            self.led.insert(*k, a.clone());
        }
    }
    fn data(&self, k: &Pubkey) -> &[u8] {
        &self.led.get(k).unwrap().data
    }
}

#[test]
fn idle_player_is_eliminated_actors_survive() {
    let mut h = H::new();
    let round = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    h.put(round, owned(68));
    h.put(authority, wallet());

    // open the round
    h.ok(vec![w(round), sgn(authority)], vec![0u8, 0u8]);

    // three players, each with a distinct owner
    let owners: Vec<Pubkey> = (0..3).map(|_| Pubkey::new_unique()).collect();
    let mut players = vec![];
    for (i, owner) in owners.iter().enumerate() {
        let (p, bump) = Pubkey::find_program_address(
            &[b"ply", round.as_ref(), &(i as u64).to_le_bytes()],
            &pid(),
        );
        h.put(p, owned(93));
        h.put(*owner, wallet());
        let mut d = vec![1u8, bump];
        d.extend_from_slice(owner.as_ref());
        h.ok(vec![w(round), w(p), sgn(authority)], d);
        players.push(p);
    }

    // lock → action window open
    h.ok(vec![w(round), sgn(authority)], vec![2u8]);

    // players 0 and 1 act; player 2 stays offline (never sends a tx)
    h.ok(vec![ro(round), w(players[0]), sgn(owners[0])], vec![3u8]);
    h.ok(vec![ro(round), w(players[1]), sgn(owners[1])], vec![3u8]);

    // permissionless sweep over the whole roster, then resolve the round
    h.ok(
        vec![w(round), w(players[0]), w(players[1]), w(players[2])],
        vec![4u8, 3u8],
    );
    h.ok(vec![w(round)], vec![5u8]);

    // ── the point ──
    assert_eq!(h.data(&round)[STATUS_OFFSET], STATUS_DONE, "round resolved");
    assert_eq!(h.data(&players[0])[ALIVE_OFFSET], 1, "actor survives");
    assert_eq!(h.data(&players[1])[ALIVE_OFFSET], 1, "actor survives");
    assert_eq!(
        h.data(&players[2])[ALIVE_OFFSET],
        0,
        "offline player eliminated by the crank — the push-mode effect pull can't express"
    );
}

const STATUS_DONE: u8 = 4;
