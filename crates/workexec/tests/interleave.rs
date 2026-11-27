//! Native-only evidence for the rust-16 gate "0 duplicate dispatches in 1,000
//! interleavings": two racing passes over one synthetic data root never both
//! hold the pass lock, and the loser always skips.

use careerbrew_workexec::watch;
use std::sync::{Arc, Barrier};

#[test]
fn two_racing_passes_never_both_hold_the_pass_lock() {
    let scratch = tempfile::tempdir().unwrap();
    let root = Arc::new(scratch.path().join("data"));
    std::fs::create_dir_all(root.as_path()).unwrap();
    let mut duplicates = 0;
    let mut idle = 0;
    for _ in 0..1000 {
        let start = Arc::new(Barrier::new(2));
        let held = Arc::new(Barrier::new(2));
        let racers = (0..2)
            .map(|_| {
                let (root, start, held) = (root.clone(), start.clone(), held.clone());
                std::thread::spawn(move || {
                    start.wait();
                    let lock = watch::try_pass_lock(&root).unwrap();
                    let won = lock.is_some();
                    // Both racers try before either releases its handle.
                    held.wait();
                    drop(lock);
                    won
                })
            })
            .collect::<Vec<_>>();
        let winners = racers
            .into_iter()
            .map(|racer| racer.join().unwrap())
            .filter(|won| *won)
            .count();
        duplicates += usize::from(winners > 1);
        idle += usize::from(winners == 0);
    }
    assert_eq!(duplicates, 0);
    assert_eq!(idle, 0);
}
