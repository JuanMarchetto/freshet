//! §1 — the REDUCE accumulator. A `Monoid` MUST be associative + commutative so shards
//! merge in any order/count deterministically (the §7 sharding contract); `IDENTITY`
//! seeds `acc_global`/`acc_partial` (explicit init — zeroed bytes are NOT assumed equal
//! to IDENTITY for non-zero monoids). Concrete monoids the demos use live here.

/// The on-chain accumulator window (`acc_global`/`acc_partial` in SPEC §2). A monoid is
/// serialized into this fixed 64-byte buffer (LE, zero-padded) — `from_acc_bytes` MUST
/// invert `to_acc_bytes`. The 64-byte cap is the inline-acc limit (§2.5; `acc_external`
/// is deferred).
pub const ACC_BYTES: usize = 64;

pub trait Monoid: Copy + PartialEq + core::fmt::Debug {
    const IDENTITY: Self;
    fn combine(self, other: Self) -> Self;
    /// Serialize into the 64-byte acc window (LE, zero-pad the tail).
    fn to_acc_bytes(&self) -> [u8; ACC_BYTES];
    /// Deserialize from the 64-byte acc window. Must round-trip `to_acc_bytes`.
    fn from_acc_bytes(b: &[u8; ACC_BYTES]) -> Self;
}

/// Saturating sum (pot total, vote tally, headcount).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sum(pub u64);

impl Monoid for Sum {
    const IDENTITY: Self = Sum(0);
    fn combine(self, other: Self) -> Self {
        Sum(self.0.saturating_add(other.0))
    }
    fn to_acc_bytes(&self) -> [u8; ACC_BYTES] {
        let mut out = [0u8; ACC_BYTES];
        out[0..8].copy_from_slice(&self.0.to_le_bytes());
        out
    }
    fn from_acc_bytes(b: &[u8; ACC_BYTES]) -> Self {
        let mut w = [0u8; 8];
        w.copy_from_slice(&b[0..8]);
        Sum(u64::from_le_bytes(w))
    }
}

/// Highest score + the winning member's global index. Ties resolve to the LOWER index
/// (deterministic ⇒ commutative). IDENTITY = no-entry (score 0, winner sentinel MAX).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MaxWinner {
    pub score: u64,
    pub winner: u64,
}

impl Monoid for MaxWinner {
    const IDENTITY: Self = MaxWinner {
        score: 0,
        winner: u64::MAX,
    };
    fn combine(self, other: Self) -> Self {
        if other.score > self.score {
            other
        } else if other.score < self.score {
            self
        } else if other.winner < self.winner {
            other // tie on score → lower index wins (deterministic ⇒ commutative)
        } else {
            self
        }
    }
    fn to_acc_bytes(&self) -> [u8; ACC_BYTES] {
        let mut out = [0u8; ACC_BYTES];
        out[0..8].copy_from_slice(&self.score.to_le_bytes());
        out[8..16].copy_from_slice(&self.winner.to_le_bytes());
        out
    }
    fn from_acc_bytes(b: &[u8; ACC_BYTES]) -> Self {
        let mut s = [0u8; 8];
        let mut w = [0u8; 8];
        s.copy_from_slice(&b[0..8]);
        w.copy_from_slice(&b[8..16]);
        MaxWinner {
            score: u64::from_le_bytes(s),
            winner: u64::from_le_bytes(w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_laws<M: Monoid>(samples: &[M]) {
        for &a in samples {
            // identity
            assert_eq!(a.combine(M::IDENTITY), a, "right identity for {a:?}");
            assert_eq!(M::IDENTITY.combine(a), a, "left identity for {a:?}");
            for &b in samples {
                // commutativity
                assert_eq!(a.combine(b), b.combine(a), "commutativity {a:?} {b:?}");
                for &c in samples {
                    // associativity
                    assert_eq!(
                        a.combine(b).combine(c),
                        a.combine(b.combine(c)),
                        "associativity {a:?} {b:?} {c:?}"
                    );
                }
            }
        }
    }

    fn assert_acc_roundtrip<M: Monoid>(v: M) {
        assert_eq!(
            M::from_acc_bytes(&v.to_acc_bytes()),
            v,
            "acc-bytes round-trip {v:?}"
        );
    }

    #[test]
    fn identity_and_values_roundtrip_through_acc_bytes() {
        assert_acc_roundtrip(Sum::IDENTITY);
        assert_acc_roundtrip(Sum(123_456_789));
        assert_acc_roundtrip(MaxWinner::IDENTITY);
        assert_acc_roundtrip(MaxWinner {
            score: 99,
            winner: 7,
        });
        // The IDENTITY of MaxWinner is NOT all-zeroes (winner sentinel = u64::MAX) — the
        // bug class the explicit-IDENTITY-init rule exists to prevent.
        assert_eq!(MaxWinner::IDENTITY.winner, u64::MAX);
        assert_ne!(MaxWinner::IDENTITY.to_acc_bytes(), [0u8; ACC_BYTES]);
    }

    #[test]
    fn sum_is_a_monoid() {
        let s: Vec<Sum> = [0u64, 1, 7, 42, 1000].iter().map(|&x| Sum(x)).collect();
        check_laws(&s);
        assert_eq!(Sum(3).combine(Sum(4)), Sum(7));
    }

    #[test]
    fn sum_saturates() {
        assert_eq!(Sum(u64::MAX).combine(Sum(5)), Sum(u64::MAX));
    }

    #[test]
    fn maxwinner_is_a_monoid() {
        let s: Vec<MaxWinner> = [(0u64, 9u64), (5, 1), (5, 3), (8, 2), (8, 0)]
            .iter()
            .map(|&(score, winner)| MaxWinner { score, winner })
            .collect();
        check_laws(&s);
    }

    #[test]
    fn reduce_is_invariant_under_shard_count() {
        // §14 shard-count invariance: sharded REDUCE+merge == single fold, for ALL P.
        // MaxWinner with ties + zero scores exercises the non-zero IDENTITY (winner=MAX):
        // a memzeroed partial would elect index 0 and break this.
        use crate::partition::{partition_len, partition_start};
        let scores = [3u64, 0, 9, 9, 1, 0, 5, 9, 2, 0];
        let total = scores.len() as u64;
        let member = |i: u64| MaxWinner {
            score: scores[i as usize],
            winner: i,
        };

        let mut single = MaxWinner::IDENTITY;
        for i in 0..total {
            single = single.combine(member(i));
        }
        assert_eq!(
            single,
            MaxWinner {
                score: 9,
                winner: 2
            },
            "max 9, lowest tied index 2"
        );

        for p in 1..=total {
            let mut acc = MaxWinner::IDENTITY; // merge accumulator
            for sid in 0..p {
                let start = partition_start(total, p, sid);
                let len = partition_len(total, p, sid);
                let mut partial = MaxWinner::IDENTITY; // per-shard, explicit IDENTITY seed
                for j in 0..len {
                    partial = partial.combine(member(start + j));
                }
                acc = acc.combine(partial);
            }
            assert_eq!(acc, single, "REDUCE result must be invariant under P={p}");
        }
    }

    #[test]
    fn maxwinner_picks_higher_score_then_lower_index() {
        let a = MaxWinner {
            score: 5,
            winner: 3,
        };
        let b = MaxWinner {
            score: 8,
            winner: 7,
        };
        assert_eq!(a.combine(b), b, "higher score wins");
        let c = MaxWinner {
            score: 8,
            winner: 2,
        };
        assert_eq!(b.combine(c), c, "tie -> lower index wins");
    }
}
