//! §2.8 — deterministic member→shard partition (computed, never stored).
//!
//! `seal` asserts `1 <= shards <= total`, so every shard has `len >= 1`
//! (no empty-shard wedge). All arithmetic is u64.

/// Number of members owned by shard `shard_id` of `shards` partitions over `total`.
pub fn partition_len(total: u64, shards: u64, shard_id: u64) -> u64 {
    let base = total / shards;
    let rem = total % shards;
    base + if shard_id < rem { 1 } else { 0 }
}

/// First global member index owned by shard `shard_id`.
pub fn partition_start(total: u64, shards: u64, shard_id: u64) -> u64 {
    let base = total / shards;
    let rem = total % shards;
    shard_id * base + shard_id.min(rem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uneven_partition_tiles_range_exactly() {
        // total=10 over P=3: base=3, rem=1 -> lens [4,3,3], starts [0,4,7].
        let total = 10;
        let p = 3;
        assert_eq!(partition_len(total, p, 0), 4);
        assert_eq!(partition_len(total, p, 1), 3);
        assert_eq!(partition_len(total, p, 2), 3);
        assert_eq!(partition_start(total, p, 0), 0);
        assert_eq!(partition_start(total, p, 1), 4);
        assert_eq!(partition_start(total, p, 2), 7);
    }

    /// Asserts the P shards exactly tile [0, total): contiguous, no gap/overlap,
    /// every shard non-empty (the §2.8 + seal `1<=P<=total` guarantee).
    fn assert_tiles(total: u64, p: u64) {
        assert_eq!(
            partition_start(total, p, 0),
            0,
            "must start at 0 (total={total} p={p})"
        );
        let mut covered = 0;
        for id in 0..p {
            let len = partition_len(total, p, id);
            assert!(
                len >= 1,
                "empty shard {id} (total={total} p={p}) — would wedge the phase"
            );
            assert_eq!(
                partition_start(total, p, id),
                covered,
                "shard {id} start must be contiguous (total={total} p={p})"
            );
            covered += len;
        }
        assert_eq!(
            covered, total,
            "shards must cover exactly total (total={total} p={p})"
        );
    }

    #[test]
    fn partition_tiles_exactly_across_many_shapes() {
        for total in 1..=200u64 {
            for p in 1..=total {
                assert_tiles(total, p);
            }
        }
    }

    #[test]
    fn edge_shapes() {
        assert_tiles(1, 1); // single member, single shard
        assert_tiles(100, 1); // one shard owns everything
        assert_tiles(100, 100); // every shard owns exactly one
        assert_tiles(7, 7);
    }

    #[test]
    fn no_overflow_for_large_u64_totals() {
        // `shard_id * base` could overflow in principle, but shard_id < p <= total and
        // base = total/p, so shard_id*base <= total; never overflows for total in u64.
        let total = u64::MAX / 2 + 12345;
        for &p in &[1u64, 2, 3, 7, 1000, 999_983] {
            assert_tiles_endpoints(total, p);
        }
    }

    /// Lightweight tiling check (endpoints + a few interior shards) for huge totals
    /// where iterating all P shards would be too slow.
    fn assert_tiles_endpoints(total: u64, p: u64) {
        assert_eq!(partition_start(total, p, 0), 0);
        let last = p - 1;
        assert_eq!(
            partition_start(total, p, last) + partition_len(total, p, last),
            total,
            "last shard must reach total (total={total} p={p})"
        );
        for id in [0, p / 2, last] {
            if id + 1 < p {
                assert_eq!(
                    partition_start(total, p, id) + partition_len(total, p, id),
                    partition_start(total, p, id + 1),
                    "contiguity at shard {id} (total={total} p={p})"
                );
            }
            assert!(partition_len(total, p, id) >= 1);
        }
    }
}
