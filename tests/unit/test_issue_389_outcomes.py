import json
from contextlib import contextmanager
from datetime import date
from pathlib import Path
from queue import SimpleQueue
from threading import Event, Thread, current_thread

import pytest

from careerbrew import actions, cli, store
from careerbrew.day import log as daylog
from careerbrew.day import pm_learn, pm_promises
from careerbrew.work import feed, projects, promises

HISTORIES = Path(__file__).resolve().parents[1] / "fixtures/pm-promise-outcomes/histories.json"


def test_promise_accept_form_uses_one_confirmation():
    action = next(row for row in actions.catalog() if row["name"] == "work.promise.accept")
    assert action["confirm"] and "work.promise.accept" in actions.YES_FROM_CONFIRM
    with pytest.raises(actions.ActionError, match="confirm to run"):
        actions.run("work.promise.accept", {"proposal_id": "synthetic"})


def test_sent_moved_identity(monkeypatch, tmp_path):
    data = json.loads(HISTORIES.read_text(encoding="utf-8"))
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    original = promises.open_list()
    assert len(original) == 1
    observed = []
    for _ in range(data["O02"]["replay"]):
        pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
        observed.append(sorted((row["name"], row["due"]) for row in promises.open_list()))

    (tmp_path / "synthetic-promise-identities.json").write_text(json.dumps(observed), encoding="utf-8")
    assert observed[0] == observed[1]
    assert [name for name, _ in observed[0]] == [original[0]["name"]]


def test_next_day_view_uses_moved_promise_identity(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    view = pm_promises.view(date(2026, 11, 3))
    assert [row["promise"] for row in view["items"]] == [original]
    assert view["items"][0]["recorded"]


def test_ambiguous_revision_keeps_existing_same_day_promise(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"])
    before = {row["name"] for row in promises.open_list()}
    assert len(before) == 2 and original in before
    pm_learn.save_sent(data["next_day"], data["O04"]["sent"], receipt=data["next_receipt"])
    assert before <= {row["name"] for row in promises.open_list()}


@pytest.mark.parametrize("failure", ["text", "receipt", "binding", "projects"])
def test_interrupted_sent_revision_cannot_accept_old_proposal(failure, monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    old = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    write_json = store.write_json_if_changed
    write_text = store.write_text_if_changed
    record = pm_promises.record

    def interrupted_write(path, value):
        target = pm_learn.receipt_path(data["next_day"]) if failure == "receipt" else pm_learn._outcome_path(data["next_day"])
        if failure != "projects" and path == target:
            raise OSError("synthetic interruption")
        return write_json(path, value)

    def interrupted_text(path, value):
        if failure == "text" and path == pm_learn.sent_path(data["next_day"]):
            raise OSError("synthetic interruption")
        return write_text(path, value)

    def interrupted_record(*args, **kwargs):
        if failure == "projects":
            raise OSError("synthetic interruption")
        return record(*args, **kwargs)

    with monkeypatch.context() as patch:
        patch.setattr(store, "write_json_if_changed", interrupted_write)
        patch.setattr(store, "write_text_if_changed", interrupted_text)
        patch.setattr(pm_promises, "record", interrupted_record)
        with pytest.raises(OSError, match="synthetic interruption"):
            pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    assert not any(row["id"] == old for row in pm_promises.outcomes())
    with pytest.raises(projects.ProjectError, match="no longer current"):
        pm_promises.accept(old)
    assert not projects.records()[original]["promise"].get("closed")
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    assert next(row["state"] for row in pm_promises.outcomes() if row["promise"] == original) == "moved"


def test_reverting_sent_text_does_not_revive_an_old_proposal(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    old = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    new = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    assert old != new
    with pytest.raises(projects.ProjectError, match="no longer current"):
        pm_promises.accept(old)
    assert not projects.records()[original]["promise"].get("closed")


@pytest.mark.parametrize("case,corrected", [("O01", "O03"), ("O02", "O01")])
def test_corrected_source_surfaces_an_accepted_decision_for_review(case, corrected, monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data[case]["sent"], receipt=data["next_receipt"])
    proposal = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    pm_promises.accept(proposal)
    accepted = projects.records()[original]
    pm_learn.save_sent(data["next_day"], data[corrected]["sent"], receipt=data["next_receipt"])
    (warning,) = [row for row in pm_promises.outcomes() if row["promise"] == original]
    assert warning["state"] == "decision-needs-review" and not warning["id"]
    after = projects.records()[original]
    assert after.get("archived_at") == accepted.get("archived_at")
    assert after["promise"]["due"] == accepted["promise"]["due"]
    assert after["promise"]["pm_decision"] == accepted["promise"]["pm_decision"]
    counts = promises.stats(7)
    assert counts["kept_on_time"] == counts["kept_late"] == counts["moved"] == 0


def test_accept_waits_for_sent_revision_and_refuses_stale_id(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    old = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    entered, release, started, finished = Event(), Event(), Event(), Event()
    errors = SimpleQueue()
    bind = pm_learn._record_outcome_revision

    def paused_binding(*args):
        entered.set()
        if not release.wait(8):
            raise TimeoutError("synthetic writer was not released")
        return bind(*args)

    def writer():
        try:
            pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
        except BaseException as error:
            errors.put(error)

    def accept():
        started.set()
        try:
            pm_promises.accept(old)
            errors.put(AssertionError("stale proposal was accepted"))
        except projects.ProjectError:
            pass
        except BaseException as error:
            errors.put(error)
        finally:
            finished.set()

    monkeypatch.setattr(pm_learn, "_record_outcome_revision", paused_binding)
    writing = Thread(target=writer, daemon=True)
    deciding = Thread(target=accept, daemon=True)
    writing.start()
    try:
        assert entered.wait(8)
        deciding.start()
        assert started.wait(8)
        assert not finished.wait(0.1)
    finally:
        release.set()
        writing.join(8)
        if deciding.ident is not None:
            deciding.join(8)
    assert not writing.is_alive() and not deciding.is_alive()
    if not errors.empty():
        raise errors.get()
    assert not projects.records()[original]["promise"].get("closed")


def test_changed_receipt_retires_proposal_and_damaged_binding_fails_closed(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    old = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    receipt = {**data["next_receipt"], "ts": "1793638800.000003"}
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=receipt)
    new = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    assert old != new
    with pytest.raises(projects.ProjectError, match="no longer current"):
        pm_promises.accept(old)
    store.write_json(pm_learn._outcome_path(data["next_day"]), {"revision": "synthetic invalid"})
    assert pm_promises.outcomes() == []
    with pytest.raises(projects.ProjectError, match="no longer current"):
        pm_promises.accept(new)
    assert not projects.records()[original]["promise"].get("closed")


def test_newer_retained_sent_text_invalidates_old_proposal(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    old = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    monkeypatch.setattr(pm_learn, "_pm_sent_text", lambda day: data["O02"]["sent"])
    assert pm_promises.outcomes() == []
    with pytest.raises(projects.ProjectError, match="no longer current"):
        pm_promises.accept(old)
    assert not projects.records()[original]["promise"].get("closed")


def test_replaying_older_day_cannot_replace_newer_observation(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    later = "2026-11-03"
    later_receipt = {"channel": "C389FIXTURE", "ts": "1793725200.000004"}
    pm_learn.save_sent(later, data["O02"]["sent"], receipt=later_receipt)
    before = next(row for row in pm_promises.outcomes() if row["promise"] == original)
    assert before["state"] == "moved"
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    assert next(row for row in pm_promises.outcomes() if row["promise"] == original) == before
    pm_promises.accept(before["id"])
    accepted = projects.records()[original]["promise"]
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    assert projects.records()[original]["promise"] == accepted
    assert not any(row["promise"] == original for row in pm_promises.outcomes())


def test_feed_writer_cannot_replace_message_during_accept(monkeypatch):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    proposal = next(row["id"] for row in pm_promises.outcomes() if row["promise"] == original)
    entered, release, writer_started, writer_acquired = Event(), Event(), Event(), Event()
    errors = SimpleQueue()
    apply = projects.accept_pm_outcome
    feed_lock = feed._locked

    @contextmanager
    def observed_lock():
        if current_thread().name == "synthetic-feed-writer":
            writer_started.set()
        with feed_lock():
            if current_thread().name == "synthetic-feed-writer":
                writer_acquired.set()
            yield

    def paused_apply(*args):
        entered.set()
        if not release.wait(8):
            raise TimeoutError("synthetic decision was not released")
        return apply(*args)

    def accept():
        try:
            pm_promises.accept(proposal)
        except BaseException as error:
            errors.put(error)

    def writer():
        try:
            feed.ingest(
                [
                    {
                        "id": f"slack:{data['next_receipt']['channel']}:{data['next_receipt']['ts']}",
                        "source": "slack",
                        "kind": "message",
                        "at": "2026-11-02T17:00:00Z",
                        "title": "Synthetic update",
                        "actor": "me",
                        "channel": "example",
                        "text": data["O02"]["sent"],
                    }
                ]
            )
        except BaseException as error:
            errors.put(error)

    monkeypatch.setattr(projects, "accept_pm_outcome", paused_apply)
    monkeypatch.setattr(feed, "_locked", observed_lock)
    deciding = Thread(target=accept, daemon=True)
    writing = Thread(target=writer, name="synthetic-feed-writer", daemon=True)
    deciding.start()
    try:
        assert entered.wait(8)
        writing.start()
        assert writer_started.wait(8)
        assert not writer_acquired.wait(0.1)
    finally:
        release.set()
        deciding.join(8)
        if writing.ident is not None:
            writing.join(8)
    assert not deciding.is_alive() and not writing.is_alive() and writer_acquired.is_set()
    if not errors.empty():
        raise errors.get()
    assert pm_learn.pm_sent_text(data["next_day"]) == data["O02"]["sent"]


def _setup(monkeypatch):
    data = json.loads(HISTORIES.read_text(encoding="utf-8"))
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    (original,) = promises.open_list()
    return data, original["name"]


def _command(capsys, *argv):
    code = cli.main(list(argv))
    output = capsys.readouterr()
    assert output.err == ""
    return code, output.out


def _outcomes(capsys):
    code, output = _command(capsys, "work", "promise", "outcomes", "--json")
    assert code == 0
    assert "Ship the sample" not in output and "C389FIXTURE" not in output
    return json.loads(output)["outcomes"]


@pytest.mark.parametrize(
    ("case", "state"),
    [pytest.param("O01", "done", id="O01"), pytest.param("O03", "not-mentioned", id="O03"), pytest.param("O04", "ambiguous", id="O04")],
)
def test_sent_outcome_after_only(case, state, monkeypatch, capsys, tmp_path):
    data, original = _setup(monkeypatch)
    if case == "O04":
        daylog.save_pm_draft(data["next_day"], data[case]["draft"])
        assert _outcomes(capsys) == []
    pm_learn.save_sent(data["next_day"], data[case]["sent"], receipt=data["next_receipt"])
    matching = [row for row in _outcomes(capsys) if row["promise"] == original]
    private = projects.records()[original]["promise"]
    evidence = {"states": [row["state"] for row in matching], "closed": bool(private.get("closed"))}
    (tmp_path / "issue389-outcome-observation.json").write_text(json.dumps(evidence), encoding="utf-8")
    assert evidence == {"states": [state], "closed": False}
    assert bool(matching[0].get("id")) == (case == "O01")


def test_owner_accepts_exact_revision_once_and_manual_drop_stays_distinct(monkeypatch, capsys, tmp_path):
    data, original = _setup(monkeypatch)
    pm_learn.save_sent(data["next_day"], data["O01"]["sent"], receipt=data["next_receipt"])
    (old,) = [row for row in _outcomes(capsys) if row["promise"] == original]
    pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
    (moved,) = [row for row in _outcomes(capsys) if row["promise"] == original]
    assert moved["state"] == "moved" and moved["id"] != old["id"]
    before = projects.path().read_bytes()
    denied, _ = _command(capsys, "work", "promise", "accept", moved["id"], "--yes")
    assert denied == 2 and projects.path().read_bytes() == before

    def decide(ident):
        return actions.run("work.promise.accept", {"proposal_id": ident}, confirm=True)["exit"]

    stale = decide(old["id"])
    assert projects.path().read_bytes() == before
    applied = decide(moved["id"])
    kept_bytes = projects.path().read_bytes()
    replay = decide(moved["id"])
    assert projects.path().read_bytes() == kept_bytes
    current = projects.records()[original]["promise"]
    assert current["due"] == "2026-11-03" and current["original_due"] == "2026-11-02" and not current.get("closed")
    name = promises.add("Synthetic manual drop", due="2026-11-04")
    promises.close(name, "dropped")
    evidence = {"decisions": [stale, applied, replay], "moved_still_open": not current.get("closed"), "drop": projects.records()[name]["promise"]["closed"]}
    (tmp_path / "issue389-accept-observation.json").write_text(json.dumps(evidence), encoding="utf-8")
    assert evidence == {"decisions": [2, 0, 2], "moved_still_open": True, "drop": "dropped"}


@pytest.mark.parametrize("case", ["boundary", "empty", "median"])
def test_stats_aggregate_window(case, monkeypatch, capsys):
    def make(label, day, due, closed_day):
        monkeypatch.setenv("CAREERBREW_NOW", day + "T12:00:00-04:00")
        name = promises.add(label, due=due)
        monkeypatch.setenv("CAREERBREW_NOW", closed_day + "T12:00:00-04:00")
        promises.close(name, "done")

    if case == "boundary":
        make("Synthetic outside canary", "2026-10-27", "2026-10-28", "2026-10-28")
        make("Synthetic inside canary", "2026-10-28", "2026-10-29", "2026-10-29")
    elif case == "median":
        make("Synthetic on-time canary", "2026-10-28", "2026-10-29", "2026-10-29")
        make("Synthetic late canary", "2026-10-29", "2026-10-30", "2026-11-01")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-03T12:00:00-04:00")
    code, output = _command(capsys, "work", "promise", "stats", "--days", "7", "--json")
    assert code == 0 and "canary" not in output
    assert json.loads(output) == {
        "made": {"boundary": 1, "empty": 0, "median": 2}[case],
        "kept_on_time": int(case != "empty"),
        "kept_late": int(case == "median"),
        "moved": 0,
        "dropped": 0,
        "not_mentioned": 0,
        "median_days_to_keep": {"boundary": 1, "empty": None, "median": 2}[case],
    }


def _skipped_workday_seed(monkeypatch):
    fixture = HISTORIES.with_name("skipped-workday.json")
    data = json.loads(fixture.read_bytes())
    monkeypatch.setenv("CAREERBREW_NOW", data["first_clock"])
    pm_learn.save_sent(data["first_day"], data["first_sent"], receipt=data["first_receipt"])
    (original,) = promises.open_list()
    assert original["due"] == data["missed_day"]
    assert not pm_learn.sent_path(data["missed_day"]).exists()
    assert not pm_learn.receipt_path(data["missed_day"]).exists()
    monkeypatch.setenv("CAREERBREW_NOW", data["next_clock"])
    return data, original["name"]


def _skipped_workday_state():
    return {row["name"]: row["due"] for row in promises.open_list()}


def _skipped_workday_record(tmp_path, record):
    (tmp_path / "skipped-workday-observation.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")


def test_skipped_workday_primary(monkeypatch, capsys, tmp_path):
    data, original = _skipped_workday_seed(monkeypatch)
    histories = []
    for _ in range(2):
        pm_learn.save_sent(data["next_day"], data["repeated_sent"], receipt=data["next_receipt"])
        histories.append({"open": _skipped_workday_state(), "outcomes": _outcomes(capsys)})
    moved = [row for row in histories[-1]["outcomes"] if row["promise"] == original and row["state"] == "moved" and row["due"] == data["new_due"] and row["id"]]
    decision = None
    if len(moved) == 1 and set(histories[-1]["open"]) == {original}:
        decision = actions.run("work.promise.accept", {"proposal_id": moved[0]["id"]}, confirm=True)["exit"]
    saved = projects.records()[original]["promise"]
    observed = {
        "original": original,
        "histories": histories,
        "decision": decision,
        "final_open": _skipped_workday_state(),
        "final_due": saved["due"],
        "original_due": saved.get("original_due"),
        "closed": bool(saved.get("closed")),
    }
    _skipped_workday_record(tmp_path, observed)

    assert histories[0] == histories[1]
    assert len(histories[-1]["open"]) == 1 and original in histories[-1]["open"]
    assert histories[-1]["open"] == {original: data["missed_day"]}
    assert len(histories[-1]["outcomes"]) == len(moved) == 1
    assert decision == 0 and observed["final_open"] == {original: data["new_due"]}
    assert observed["final_due"] == data["new_due"] and observed["original_due"] == data["missed_day"]
    assert not observed["closed"]


@pytest.mark.parametrize("case", ["unrelated", "ambiguous", "no-receipt"])
def test_skipped_workday_control(case, monkeypatch, capsys, tmp_path):
    data, original = _skipped_workday_seed(monkeypatch)
    if case == "ambiguous":
        # Create a second valid prior identity through the existing manual sent writer.
        # Backfill it after Friday so a new trusted matcher cannot collapse setup.
        pm_learn.save_sent(data["older_day"], data["older_sent"])
    old_open = _skipped_workday_state()
    old_rows = projects.records()
    old_identities = {old_rows[name]["promise"]["identity_digest"] for name in old_open}
    assert len(old_open) == (2 if case == "ambiguous" else 1) and len(old_identities) == 1
    before_due = {name: old_rows[name]["promise"]["due"] for name in old_open}

    text = data["unrelated_sent"] if case == "unrelated" else data["repeated_sent"]
    receipt = None if case == "no-receipt" else data["next_receipt"]
    pm_learn.save_sent(data["next_day"], text, receipt=receipt)
    after_open = _skipped_workday_state()
    rows = projects.records()
    outcomes = _outcomes(capsys)
    new_names = set(after_open) - set(old_open)
    observed = {
        "original": original,
        "prior_count": len(old_open),
        "new_count": len(new_names),
        "old_due_preserved": all(after_open.get(name) == due for name, due in before_due.items()),
        "old_unclosed": all(not rows[name]["promise"].get("closed") for name in old_open),
        "old_undecided": all(not rows[name]["promise"].get("pm_decision") for name in old_open),
        "new_due_matches": all(after_open[name] == data["new_due"] for name in new_names),
        "states": sorted(row["state"] for row in outcomes),
        "actionable": sum(bool(row["id"]) for row in outcomes),
    }
    _skipped_workday_record(tmp_path, observed)

    assert observed["old_due_preserved"] and observed["old_unclosed"] and observed["old_undecided"]
    assert observed["new_due_matches"] and observed["actionable"] == 0
    if case == "ambiguous":
        # Before may create a separate Tuesday row. A refusal may suppress it.
        assert observed["new_count"] in (0, 1) and set(observed["states"]) <= {"ambiguous"}
    else:
        assert observed["new_count"] == 1
        assert set(observed["states"]) <= ({"not-mentioned"} if case == "unrelated" else set())
