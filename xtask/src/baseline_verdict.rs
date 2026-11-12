//! Fail-closed verdicts for one set of paired migration observations.

use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Write<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy)]
pub struct ResourceLimit {
    pub floor: u64,
    pub absolute: u64,
}

pub struct Oracle<'a> {
    pub exit: i32,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub checks: &'a [&'a str],
    pub writes: &'a [Write<'a>],
    pub canaries: &'a [&'a [u8]],
    pub cpu: ResourceLimit,
    pub rss: ResourceLimit,
}

#[derive(Clone, Copy)]
pub struct Observation<'a> {
    pub exit: i32,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub checks: &'a [&'a str],
    pub writes: &'a [Write<'a>],
    pub cached: bool,
    pub cpu_ns: u64,
    pub peak_rss_bytes: u64,
}

pub struct Pair<'a> {
    pub id: u64,
    pub baseline: Observation<'a>,
    pub candidate: Observation<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Hold {
    InvalidOracle,
    Pairs,
    Contract,
    Privacy,
    Checks,
    Cpu,
    Memory,
}

fn has_canary(observation: Observation<'_>, canaries: &[&[u8]]) -> bool {
    let output = [observation.stdout, observation.stderr];
    let writes = observation
        .writes
        .iter()
        .flat_map(|write| [write.path.as_bytes(), write.bytes]);
    output.into_iter().chain(writes).any(|bytes| {
        canaries
            .iter()
            .any(|canary| bytes.windows(canary.len()).any(|window| window == *canary))
    })
}

fn cpu_p95(mut samples: Vec<u64>) -> u64 {
    let index = samples.len() - samples.len() / 20 - 1;
    *samples.select_nth_unstable(index).1
}

fn within_limit(baseline: u64, candidate: u64, limit: ResourceLimit) -> bool {
    if baseline < limit.floor {
        return candidate <= limit.absolute;
    }
    u128::from(candidate) * 10 <= u128::from(baseline) * 11
}

pub fn decide(oracle: &Oracle<'_>, pairs: &[Pair<'_>]) -> Result<(), Hold> {
    let expected_checks: BTreeSet<_> = oracle.checks.iter().copied().collect();
    if expected_checks.is_empty()
        || expected_checks.len() != oracle.checks.len()
        || expected_checks.contains("")
        || oracle.canaries.iter().any(|canary| canary.is_empty())
    {
        return Err(Hold::InvalidOracle);
    }

    let mut ids = BTreeSet::new();
    if pairs.len() < 30
        || pairs
            .iter()
            .any(|pair| pair.baseline.cached || pair.candidate.cached || !ids.insert(pair.id))
    {
        return Err(Hold::Pairs);
    }

    for observation in pairs
        .iter()
        .flat_map(|pair| [pair.baseline, pair.candidate])
    {
        if has_canary(observation, oracle.canaries) {
            return Err(Hold::Privacy);
        }
        if observation.exit != oracle.exit
            || observation.stdout != oracle.stdout
            || observation.stderr != oracle.stderr
            || observation.writes != oracle.writes
        {
            return Err(Hold::Contract);
        }
        let observed_checks: BTreeSet<_> = observation.checks.iter().copied().collect();
        if observation.checks.len() != expected_checks.len() || observed_checks != expected_checks {
            return Err(Hold::Checks);
        }
    }

    let baseline_cpu = cpu_p95(pairs.iter().map(|pair| pair.baseline.cpu_ns).collect());
    let candidate_cpu = cpu_p95(pairs.iter().map(|pair| pair.candidate.cpu_ns).collect());
    if !within_limit(baseline_cpu, candidate_cpu, oracle.cpu) {
        return Err(Hold::Cpu);
    }

    let baseline_rss = pairs
        .iter()
        .map(|pair| pair.baseline.peak_rss_bytes)
        .max()
        .unwrap();
    let candidate_rss = pairs
        .iter()
        .map(|pair| pair.candidate.peak_rss_bytes)
        .max()
        .unwrap();
    if !within_limit(baseline_rss, candidate_rss, oracle.rss) {
        return Err(Hold::Memory);
    }
    Ok(())
}

pub mod controls {
    use super::*;

    pub fn run() -> usize {
        let cases = [
            unchanged_pairs_keep,
            wrong_bytes_and_unexpected_write_hold,
            matching_canary_still_holds,
            missing_or_repeated_checks_hold,
            empty_expected_checks_hold,
            fewer_than_thirty_fresh_unique_pairs_hold,
            inclusive_ratio_limits_use_cpu_p95_and_max_rss,
            below_floor_uses_absolute_limits,
        ];
        for case in cases {
            case();
        }
        cases.len()
    }

    fn oracle() -> Oracle<'static> {
        Oracle {
            exit: 0,
            stdout: b"unchanged output\n",
            stderr: b"",
            checks: &["cli", "storage"],
            writes: &[Write {
                path: "receipt",
                bytes: b"one\n",
            }],
            canaries: &[b"synthetic-secret"],
            cpu: ResourceLimit {
                floor: 100,
                absolute: 100,
            },
            rss: ResourceLimit {
                floor: 1000,
                absolute: 1000,
            },
        }
    }

    fn pairs() -> Vec<Pair<'static>> {
        let baseline = Observation {
            exit: 0,
            stdout: b"unchanged output\n",
            stderr: b"",
            checks: &["storage", "cli"],
            writes: &[Write {
                path: "receipt",
                bytes: b"one\n",
            }],
            cached: false,
            cpu_ns: 200,
            peak_rss_bytes: 2000,
        };
        (1..=30)
            .map(|id| Pair {
                id,
                baseline,
                candidate: baseline,
            })
            .collect()
    }

    #[cfg_attr(test, test)]
    fn unchanged_pairs_keep() {
        assert_eq!(decide(&oracle(), &pairs()), Ok(()));
    }

    #[cfg_attr(test, test)]
    fn wrong_bytes_and_unexpected_write_hold() {
        let mut trials = pairs();
        trials[0].candidate.stdout = b"wrong output\n";
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Contract));
        trials[0].candidate = trials[0].baseline;
        trials[0].candidate.writes = &[];
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Contract));
    }

    #[cfg_attr(test, test)]
    fn matching_canary_still_holds() {
        let mut oracle = oracle();
        oracle.stderr = b"synthetic-secret";
        let mut trials = pairs();
        for trial in &mut trials {
            trial.baseline.stderr = oracle.stderr;
            trial.candidate.stderr = oracle.stderr;
        }
        assert_eq!(decide(&oracle, &trials), Err(Hold::Privacy));
    }

    #[cfg_attr(test, test)]
    fn missing_or_repeated_checks_hold() {
        let mut trials = pairs();
        trials[0].candidate.checks = &["cli"];
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Checks));
        trials[0].candidate.checks = &["cli", "storage", "storage"];
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Checks));
    }

    #[cfg_attr(test, test)]
    fn empty_expected_checks_hold() {
        let mut expected = oracle();
        expected.checks = &[];
        let mut trials = pairs();
        for trial in &mut trials {
            trial.baseline.checks = &[];
            trial.candidate.checks = &[];
        }
        assert_eq!(decide(&expected, &trials), Err(Hold::InvalidOracle));
    }

    #[cfg_attr(test, test)]
    fn fewer_than_thirty_fresh_unique_pairs_hold() {
        let mut trials = pairs();
        assert_eq!(decide(&oracle(), &trials[..29]), Err(Hold::Pairs));
        trials[0].candidate.cached = true;
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Pairs));
        trials[0].candidate.cached = false;
        trials[0].id = trials[1].id;
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Pairs));
    }

    #[cfg_attr(test, test)]
    fn inclusive_ratio_limits_use_cpu_p95_and_max_rss() {
        let mut trials = pairs();
        for trial in &mut trials {
            trial.candidate.cpu_ns = 220;
            trial.candidate.peak_rss_bytes = 2200;
        }
        trials[0].candidate.cpu_ns = 221;
        assert_eq!(decide(&oracle(), &trials), Ok(()));
        trials[1].candidate.cpu_ns = 221;
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Cpu));
        trials[1].candidate.cpu_ns = 220;
        trials[0].candidate.peak_rss_bytes = 2201;
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Memory));
    }

    #[cfg_attr(test, test)]
    fn below_floor_uses_absolute_limits() {
        let mut trials = pairs();
        for trial in &mut trials {
            trial.baseline.cpu_ns = 1;
            trial.baseline.peak_rss_bytes = 1;
            trial.candidate.cpu_ns = 100;
            trial.candidate.peak_rss_bytes = 1000;
        }
        assert_eq!(decide(&oracle(), &trials), Ok(()));
        trials[0].candidate.peak_rss_bytes = 1001;
        assert_eq!(decide(&oracle(), &trials), Err(Hold::Memory));
    }
}
