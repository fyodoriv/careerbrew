"""rust-05b2: the approvals store holds one lock across load-change-save. Synthetic data and temp folders only."""

from __future__ import annotations

import fcntl
import threading
from pathlib import Path

from careerbrew import approvals, store


def _lock_path() -> Path:
    p = approvals._path()
    return p.with_name(f".{p.name}.lock")


def _blocked_while_locked(action) -> bool:
    """Hold the approvals lock from another open file, run `action` in a thread; True when it waited."""
    lock_path = _lock_path()
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a") as held:
        fcntl.flock(held, fcntl.LOCK_EX)
        worker = threading.Thread(target=action)
        worker.start()
        worker.join(0.3)
        waited = worker.is_alive()
        fcntl.flock(held, fcntl.LOCK_UN)
    worker.join(10)
    assert not worker.is_alive()
    return waited


def test_a_request_waits_for_the_approvals_lock(home):
    assert _blocked_while_locked(lambda: approvals.request("synthetic ask one"))
    assert [a["title"] for a in approvals.load()] == ["synthetic ask one"]


def test_a_decision_waits_for_the_approvals_lock(home):
    item = approvals.request("synthetic ask two")
    assert _blocked_while_locked(lambda: approvals.decide(item["id"], "approved"))
    assert approvals.get(item["id"])["status"] == "approved"


def test_two_writers_that_decide_different_items_at_once_both_keep_their_decision(home, monkeypatch):
    real_write = store.write_json
    gate: dict[str, threading.Barrier] = {}

    def slow_write(path, data):
        # Unlocked, both writers load before either saves, so the second save drops the first decision.
        # Locked, the second writer waits outside, the barrier times out, and each save sees the other.
        try:
            gate["b"].wait()
        except threading.BrokenBarrierError:
            pass
        return real_write(path, data)

    monkeypatch.setattr(store, "write_json", slow_write)
    for run in range(50):
        gate["b"] = threading.Barrier(1)  # requests run alone
        a = approvals.request(f"synthetic ask a{run}")
        b = approvals.request(f"synthetic ask b{run}")
        gate["b"] = threading.Barrier(2, timeout=0.2)
        threads = [threading.Thread(target=approvals.decide, args=(x["id"], "approved")) for x in (a, b)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(10)
        stored = {r["id"]: r["status"] for r in approvals.load()}
        assert stored[a["id"]] == "approved" and stored[b["id"]] == "approved", f"lost update in run {run}"
