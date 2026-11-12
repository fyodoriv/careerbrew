//! Deterministic ordering for one benchmark case and cache regime.

const MIN_PAIRS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheRegime {
    Cold,
    Warm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arm {
    Baseline,
    Candidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlannedPair {
    pub(crate) id: u64,
    pub(crate) cache: CacheRegime,
    pub(crate) order: [Arm; 2],
}

/// Pair IDs are local to the benchmark case and cache regime.
pub(crate) fn plan(
    pairs: u64,
    cache: CacheRegime,
) -> Result<impl Iterator<Item = PlannedPair>, &'static str> {
    if pairs < MIN_PAIRS {
        return Err("a trial plan needs at least 30 pairs per cache regime");
    }
    Ok((1..=pairs).map(move |id| {
        let baseline_first = (id % 2 == 1) == (cache == CacheRegime::Cold);
        let order = if baseline_first {
            [Arm::Baseline, Arm::Candidate]
        } else {
            [Arm::Candidate, Arm::Baseline]
        };
        PlannedPair { id, cache, order }
    }))
}

pub(crate) mod controls {
    use super::*;
    use Arm::{Baseline, Candidate};
    use CacheRegime::{Cold, Warm};

    pub(crate) fn run() -> usize {
        let controls = [
            rejects_insufficient_pairs,
            creates_contiguous_pair_ids,
            alternates_both_arms,
            balances_odd_pair_count,
            separates_cache_regimes,
            repeats_independently,
        ];
        for control in controls {
            control();
        }
        controls.len()
    }

    #[cfg_attr(test, test)]
    fn rejects_insufficient_pairs() {
        for cache in [Cold, Warm] {
            for pairs in [0, 29] {
                assert!(plan(pairs, cache).is_err());
            }
        }
    }

    #[cfg_attr(test, test)]
    fn creates_contiguous_pair_ids() {
        for cache in [Cold, Warm] {
            let ids: Vec<_> = plan(30, cache).unwrap().map(|pair| pair.id).collect();
            assert_eq!(ids, (1..=30).collect::<Vec<_>>());
        }
    }

    #[cfg_attr(test, test)]
    fn alternates_both_arms() {
        let expected = [[Baseline, Candidate], [Candidate, Baseline]].repeat(15);
        let orders: Vec<_> = plan(30, Cold).unwrap().map(|pair| pair.order).collect();
        assert_eq!(orders, expected);
    }

    #[cfg_attr(test, test)]
    fn balances_odd_pair_count() {
        for (cache, expected) in [(Cold, 16), (Warm, 15)] {
            let baseline_first = plan(31, cache)
                .unwrap()
                .filter(|pair| pair.order[0] == Baseline)
                .count();
            assert_eq!(baseline_first, expected);
            assert_eq!(baseline_first.abs_diff(31 - baseline_first), 1);
        }
    }

    #[cfg_attr(test, test)]
    fn separates_cache_regimes() {
        for (cold, warm) in plan(30, Cold).unwrap().zip(plan(30, Warm).unwrap()) {
            assert_eq!(cold.id, warm.id);
            assert_eq!(cold.cache, Cold);
            assert_eq!(warm.cache, Warm);
            assert_eq!(cold.order, [warm.order[1], warm.order[0]]);
        }
    }

    #[cfg_attr(test, test)]
    fn repeats_independently() {
        let mut first = plan(31, Cold).unwrap();
        let mut second = plan(31, Cold).unwrap();
        let mut warm = plan(31, Warm).unwrap();
        for _ in 1..=31 {
            let pair = first.next().unwrap();
            assert_eq!(warm.next().unwrap().id, pair.id);
            assert_eq!(second.next(), Some(pair));
        }
        assert_eq!(first.next(), None);
        assert_eq!(second.next(), None);
        assert_eq!(warm.next(), None);
    }
}
