"""Issue #30: the complexity check fits growth over 3+ sizes from the fastest of several runs.

Gate flake 2026-10-22: each run is timed in CPU time, so time spent waiting for a CPU on a busy machine is not growth.
"""

import pytest

from careerbrew.tutor import exercises


def test_slope_of_known_growth():
    assert exercises.growth_slope([(1000, 1.0), (10000, 10.0), (100000, 100.0)]) == pytest.approx(1.0)
    assert exercises.growth_slope([(1000, 1.0), (10000, 100.0), (100000, 10000.0)]) == pytest.approx(2.0)


def test_noisy_linear_timings_pass_and_quadratic_fails():
    ex = {"scale": {"expected": "O(n)"}}
    noisy_linear = [{"n": 20000, "ms": 14.6}, {"n": 63246, "ms": 60.0}, {"n": 200000, "ms": 380.0}]  # big run 26x the small one
    assert exercises._complexity(ex, noisy_linear)["ok"], "a 26x ratio from allocation cost is still linear growth"
    quadratic = [{"n": 2000, "ms": 10.0}, {"n": 6325, "ms": 100.0}, {"n": 20000, "ms": 1000.0}]
    assert not exercises._complexity(ex, quadratic)["ok"]


def test_two_sizes_become_three_and_each_size_runs_several_times(monkeypatch):
    monkeypatch.delenv("CAREERBREW_EXERCISES", raising=False)  # the real deck, not the fixtures
    assert exercises._scale_sizes([20000, 200000]) == [20000, 63246, 200000]
    ex = exercises.get("copy-list-with-random-pointer")  # the exercise from issue #30
    out = exercises.run(ex, ex["solution"])
    assert len(out["scale"]) == 3, out["scale"]  # a failure shows the child's reason (task exercises-slope-noise)
    assert all(len(s["runs"]) == exercises.SCALE_REPEATS and s["ms"] == min(s["runs"]) for s in out["scale"])


# A busy machine, made deterministic: a run longer than one time slice waits for a CPU twice as long as it
# ran, and a short run does not wait. Sleeping is exactly that to a clock: wall time passes, no CPU is used.
BUSY = """
import time

def unique_paths(m, n):
    used = time.process_time()
    ways = _unique_paths(m, n)
    used = time.process_time() - used
    if used > 0.01:
        time.sleep(2 * used)
    return ways
"""


def test_a_correct_solution_passes_on_a_busy_machine(monkeypatch):
    """Gate flake 2026-10-22: at load ~50, unique-paths measured n^2.54 against the O(n^2) limit 2.5."""
    monkeypatch.delenv("CAREERBREW_EXERCISES", raising=False)
    ex = exercises.get("unique-paths")
    code = ex["solution"].replace("def unique_paths(", "def _unique_paths(") + BUSY
    cx = exercises._complexity(ex, exercises.run(ex, code)["scale"])
    assert cx["ok"], f"time spent waiting for a CPU counted as growth: n^{cx['slope']}"


def test_a_busy_machine_cannot_time_out_a_correct_scaling_run(monkeypatch):
    """Gate flake 2026-10-22: copy-list-with-random-pointer failed with growth n^None at load ~150. Its scaling
    run uses under 3 s of CPU (limit 6 s) but waited past the 15 s wall limit. Scaled down here: each of the 5
    largest runs waits 0.8 s for a CPU, and the wall limit is 3 s."""
    monkeypatch.delenv("CAREERBREW_EXERCISES", raising=False)
    monkeypatch.setattr(exercises, "WALL_SECONDS", 3)
    ex = exercises.get("unique-paths")
    wait = "\nimport time\n\ndef unique_paths(m, n):\n    if max(m, n) >= 1000:\n        time.sleep(0.8)\n    return _unique_paths(m, n)\n"
    code = ex["solution"].replace("def unique_paths(", "def _unique_paths(") + wait
    cx = exercises._complexity(ex, exercises.run(ex, code)["scale"])
    assert cx["ok"] and cx["slope"] is not None, f"a wait for a CPU failed the run as too slow: {cx}"


def test_the_scaling_runs_together_may_use_more_cpu_than_one_run(monkeypatch):
    """Gate flake 2026-10-25: word-search-ii's reference solution failed with growth n^None. Its 15 timed runs use
    7.3 s of CPU in all (about 1 s the largest), past the 6 s per-process limit. Scaled down here: the 5 largest
    runs burn 0.6 s each (3 s in all), the one-run limit is 2 s, and the scaling run's own budget is 10 s."""
    monkeypatch.delenv("CAREERBREW_EXERCISES", raising=False)
    monkeypatch.setattr(exercises, "CPU_SECONDS", 2)
    monkeypatch.setattr(exercises, "SCALE_CPU_SECONDS", 10, raising=False)
    ex = exercises.get("unique-paths")
    burn = "\nimport time\n\ndef unique_paths(m, n):\n    if max(m, n) >= 1000:\n        end = time.process_time() + 0.6\n        while time.process_time() < end:\n            pass\n    return _unique_paths(m, n)\n"
    cx = exercises._complexity(ex, exercises.run(ex, ex["solution"].replace("def unique_paths(", "def _unique_paths(") + burn)["scale"])
    assert cx["slope"] is not None, f"the runs' total CPU killed a run that is fast each time: {cx}"


def test_a_hang_in_the_scaling_run_is_still_stopped(monkeypatch):
    monkeypatch.delenv("CAREERBREW_EXERCISES", raising=False)
    monkeypatch.setattr(exercises, "SCALE_WALL_SECONDS", 2)
    ex = exercises.get("unique-paths")
    hang = "\nimport time\n\ndef unique_paths(m, n):\n    if max(m, n) >= 1000:\n        time.sleep(60)\n    return _unique_paths(m, n)\n"
    out = exercises.run(ex, ex["solution"].replace("def unique_paths(", "def _unique_paths(") + hang)
    assert not exercises._complexity(ex, out["scale"])["ok"]


def test_a_cubic_solution_still_fails_the_quadratic_check():
    ex = {
        "function": "triples",
        "tests": [{"args": [2], "expected": 8}, {"args": [3], "expected": 27}],
        "scale": {"generator": "def make(n):\n    return [n]\n", "sizes": [40, 160], "expected": "O(n^2)"},
    }
    cubic = "def triples(n):\n    c = 0\n    for i in range(n):\n        for j in range(n):\n            for k in range(n):\n                c += 1\n    return c\n"
    cx = exercises._complexity(ex, exercises.run(ex, cubic)["scale"])
    assert not cx["ok"] and cx["slope"] > exercises.COMPLEXITY_SLOPE["O(n^2)"], cx
