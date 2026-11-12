"""Prior-day session handoffs stay available without leaking restricted text."""

import json
import subprocess
import sys
import threading
from types import SimpleNamespace

import pytest

from careerbrew import cli, config, events, store
from careerbrew.work import projects, sessions

CANARY = "SYNTHETIC-PRIOR-SESSION-HANDOFF"


@pytest.fixture(autouse=True)
def project(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-27T09:00+00:00")
    assert cli.main(["init"]) == 0
    assert cli.main(["work", "project", "add", "billing"]) == 0


def _prior(monkeypatch, day):
    monkeypatch.setenv("CAREERBREW_NOW", f"{day}T09:00+00:00")
    sessions.open_session("billing", "old-session")
    sessions.write_handoff("old-session", CANARY)
    sessions.end("old-session")


@pytest.mark.parametrize(("before", "after"), [("2026-10-27", "2026-10-28"), ("2026-12-31", "2027-01-01")])
def test_prior_day_handoff_is_the_reopened_session_brief(monkeypatch, before, after):
    _prior(monkeypatch, before)
    monkeypatch.setenv("CAREERBREW_NOW", f"{after}T09:00+00:00")
    assert sessions.open_session("billing", "new-session")["brief"] == CANARY


def test_session_loop_emits_one_content_free_added_event(monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    sessions.open_session("billing", "synthetic-session")
    sessions.open_session("billing", "synthetic-session")
    records = [json.loads(line) for line in events.log_path().read_text().splitlines()]
    added = [row for row in records if row.get("event") == "workday.loop" and row.get("status") == "added"]
    assert len(added) == 1
    assert "synthetic-session" not in json.dumps(added) and "billing" not in json.dumps(added)


def test_restricted_open_hides_prior_day_handoff(monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    _prior(monkeypatch, "2026-10-27")
    capsys.readouterr()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["work", "session", "open", "billing", "--id", "new-session", "--json"]) == 0
    output = capsys.readouterr().out
    assert CANARY not in output
    assert "brief" not in json.loads(output)
    assert CANARY not in events.log_path().read_text()


def test_older_nonquiet_handoff_is_not_added_as_a_prior_day_brief(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    assert sessions.open_session("billing", "new-session")["brief"] == ""


def test_newer_session_without_handoff_does_not_resurrect_an_older_brief(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    sessions.open_session("billing", "newer-session")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "new-session")["brief"] == ""


def test_reopening_with_a_reused_ended_id_keeps_the_prior_day_brief(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "old-session")["brief"] == CANARY


def test_authorized_open_returns_prior_day_handoff(monkeypatch, capsys):
    _prior(monkeypatch, "2026-10-27")
    capsys.readouterr()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["work", "session", "open", "billing", "--id", "new-session", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["brief"] == CANARY


def _clear_quiet():
    assert cli.main(["work", "project", "set", "billing", "--quiet-until", "2026-11-01"]) == 0
    assert cli.main(["work", "project", "set", "billing", "--quiet-until", "none"]) == 0


def test_quiet_reopen_with_same_id_preserves_existing_selection(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    _clear_quiet()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "old-session")["brief"] == ""


def test_quiet_history_does_not_revive_a_brief_over_a_newer_session(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    _clear_quiet()
    sessions.open_session("billing", "newer-session")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "new-session")["brief"] == ""


def test_quiet_generation_is_stable_for_retries_extensions_and_clear(monkeypatch):
    projects.edit("billing", quiet_until="2026-11-01")
    generation = projects.records()["billing"]["quiet_generation"]
    projects.edit("billing", quiet_until="2026-11-01")
    projects.edit("billing", quiet_until="2026-11-03")
    projects.edit("billing", quiet_until="none")
    projects.edit("billing", quiet_until="none")
    assert projects.records()["billing"]["quiet_generation"] == generation
    projects.edit("billing", quiet_until="2026-11-04")
    assert projects.records()["billing"]["quiet_generation"] != generation


def test_consumed_quiet_generation_allows_next_day_reused_id(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-29")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    first = sessions.open_session("billing", "resumed")
    assert first["brief"] == CANARY
    assert sessions.open_session("billing", "resumed")["brief"] == CANARY
    sessions.write_handoff("resumed", "SYNTHETIC-LATEST-HANDOFF")
    sessions.end("resumed")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T09:00+00:00")
    assert sessions.open_session("billing", "resumed")["brief"] == "SYNTHETIC-LATEST-HANDOFF"


def test_consumption_receipt_outlives_ended_session_pruning(monkeypatch):
    config.set_value("agent_keep_days", 1)
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-28")
    generation = projects.records()["billing"]["quiet_generation"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    sessions.open_session("billing", "resumed")
    sessions.write_handoff("resumed", "SYNTHETIC-RESUMED")
    sessions.end("resumed")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00+00:00")
    sessions.open_session("billing", "later")
    saved = store.read_json(sessions.path(), {})
    assert all(row["id"] != "resumed" for row in saved["sessions"])
    assert generation in saved["quiet_receipts"]


def test_legacy_quiet_marker_is_reported_unverified(monkeypatch, capsys):
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-29")
    data = store.read_yaml(projects.path(), {})
    data["projects"]["billing"].pop("quiet_generation", None)
    store.write_yaml(projects.path(), data)
    capsys.readouterr()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["work", "session", "open", "billing", "--id", "new-session", "--json"]) == 0
    output = capsys.readouterr().out
    parsed = json.loads(output)
    assert parsed["brief_status"] == "legacy-unverified"
    assert CANARY not in output and "brief" not in parsed


def test_competing_open_consumes_one_quiet_generation(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-28")
    generation = projects.records()["billing"]["quiet_generation"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    barrier = threading.Barrier(2)
    outcomes = []

    def attempt(ident):
        barrier.wait(timeout=5)
        try:
            outcomes.append(sessions.open_session("billing", ident))
        except sessions.SessionError:
            outcomes.append(None)

    threads = [threading.Thread(target=attempt, args=(ident,)) for ident in ("one", "two")]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(10)
    assert all(not thread.is_alive() for thread in threads)
    assert sum(row is not None for row in outcomes) == 1
    saved = store.read_json(sessions.path(), {})
    assert generation in saved["quiet_receipts"]


def test_existing_live_session_consumes_quiet_generation(monkeypatch):
    sessions.open_session("billing", "live-session")
    projects.edit("billing", quiet_until="2026-11-01")
    generation = projects.records()["billing"]["quiet_generation"]
    projects.edit("billing", quiet_until="none")
    assert sessions.open_session("billing", "live-session")["reused"]
    assert generation in store.read_json(sessions.path(), {})["quiet_receipts"]
    sessions.write_handoff("live-session", "SYNTHETIC-LIVE-HANDOFF")
    sessions.end("live-session")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "live-session")["brief"] == "SYNTHETIC-LIVE-HANDOFF"


def test_existing_live_session_receives_quiet_return_handoff(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T09:00+00:00")
    assert sessions.open_session("billing", "live-session")["brief"] == ""
    projects.edit("billing", quiet_until="2026-11-01")
    projects.edit("billing", quiet_until="none")
    assert sessions.open_session("billing", "live-session")["brief"] == CANARY
    assert sessions.open_session("billing", "live-session")["brief"] == CANARY


@pytest.mark.parametrize("after_write", [False, True])
def test_interrupted_session_save_keeps_quiet_return_retryable(monkeypatch, after_write):
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-29")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    original = store.write_json
    failed = False

    def interrupted(path, value):
        nonlocal failed
        if path == sessions.path() and not failed:
            failed = True
            if after_write:
                original(path, value)
            raise OSError("synthetic session save interruption")
        return original(path, value)

    monkeypatch.setattr(store, "write_json", interrupted)
    with pytest.raises(OSError, match="synthetic session save interruption"):
        sessions.open_session("billing", "resumed")
    assert sessions.open_session("billing", "resumed")["brief"] == CANARY


def test_interruption_after_save_before_output_keeps_selected_brief(monkeypatch):
    from careerbrew.workday import watch

    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-29")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    original = watch._save
    failed = False

    def interrupted(*args, **kwargs):
        nonlocal failed
        if not failed:
            failed = True
            raise OSError("synthetic post-save interruption")
        return original(*args, **kwargs)

    monkeypatch.setattr(watch, "_save", interrupted)
    with pytest.raises(OSError, match="synthetic post-save interruption"):
        sessions.open_session("billing", "resumed")
    assert sessions.open_session("billing", "resumed")["brief"] == CANARY
    assert [(row["kind"], row["id"]) for row in watch.status()["running"]] == [("session", "resumed")]


def test_open_refuses_a_foreign_loop_with_the_same_id():
    from careerbrew.workday import watch

    watch.add("cron", "shared-id")
    before = watch.status()["running"]
    with pytest.raises((sessions.SessionError, watch.WatchError)):
        sessions.open_session("billing", "shared-id")
    assert watch.status()["running"] == before


@pytest.mark.parametrize("after_loop_save", [False, True])
def test_day_end_accounts_for_an_interrupted_open_without_retry(monkeypatch, after_loop_save):
    from careerbrew.workday import watch

    original = watch._save

    def interrupted(*args, **kwargs):
        if after_loop_save:
            original(*args, **kwargs)
        raise OSError("synthetic open interruption")

    monkeypatch.setattr(watch, "_save", interrupted)
    with pytest.raises(OSError, match="synthetic open interruption"):
        sessions.open_session("billing", "interrupted")
    monkeypatch.setattr(watch, "_save", original)
    try:
        result = watch.end()
    except (OSError, sessions.SessionError, watch.WatchError):
        return  # failure is safe; reporting a completed day is not
    assert any(row["id"] == "interrupted" for row in result["running"])


def test_day_end_does_not_report_success_when_session_store_is_unreadable(monkeypatch):
    from careerbrew.workday import watch

    def unreadable():
        raise OSError("synthetic session store failure")

    monkeypatch.setattr(sessions, "_load", unreadable)
    with pytest.raises((OSError, sessions.SessionError, watch.WatchError)):
        watch.end()


def test_day_end_accounts_for_process_exit_after_session_save(monkeypatch):
    from careerbrew.workday import watch

    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-29")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    script = """
import os
from careerbrew import store
from careerbrew.work import sessions
write = store.write_json
def interrupted(path, value):
    write(path, value)
    if path == sessions.path():
        os._exit(91)
store.write_json = interrupted
sessions.open_session('billing', 'exited')
"""
    child = subprocess.run([sys.executable, "-c", script], capture_output=True, timeout=15, check=False)
    assert child.returncode == 91
    assert any(row["id"] == "exited" for row in watch.end()["running"])
    assert sessions.open_session("billing", "exited")["brief"] == CANARY


@pytest.mark.parametrize("boundary", ["project", "loop", "end-loop"])
def test_process_exit_at_other_save_boundaries_is_recoverable(boundary):
    from careerbrew.workday import watch

    if boundary == "project":
        data = store.read_yaml(projects.path(), {})
        data["projects"]["billing"].pop("incarnation_id")
        store.write_yaml(projects.path(), data)
    elif boundary == "end-loop":
        sessions.open_session("billing", "exited")
        sessions.write_handoff("exited", CANARY)
    script = """
import os, sys
from careerbrew import store
from careerbrew.work import sessions
from careerbrew.workday import watch
boundary = sys.argv[1]
if boundary == 'project':
    write = store.write_yaml
    def interrupted(path, value):
        write(path, value)
        os._exit(91)
    store.write_yaml = interrupted
else:
    save = watch._save
    def interrupted(value):
        save(value)
        os._exit(91)
    watch._save = interrupted
if boundary == 'end-loop':
    sessions.end('exited')
else:
    sessions.open_session('billing', 'exited')
"""
    child = subprocess.run([sys.executable, "-c", script, boundary], capture_output=True, timeout=15, check=False)
    assert child.returncode == 91
    if boundary == "end-loop":
        assert any(row["id"] == "exited" for row in watch.end()["running"])
    else:
        assert sessions.open_session("billing", "exited")["id"] == "exited"
        assert [row["id"] for row in watch.status()["running"]] == ["exited"]


@pytest.mark.parametrize("json_output", [False, True])
def test_day_end_cli_reports_interrupted_session(monkeypatch, capsys, json_output):
    from careerbrew.workday import watch

    original = watch._save

    def interrupted(*args, **kwargs):
        raise OSError("synthetic open interruption")

    monkeypatch.setattr(watch, "_save", interrupted)
    with pytest.raises(OSError):
        sessions.open_session("billing", "interrupted")
    monkeypatch.setattr(watch, "_save", original)
    capsys.readouterr()
    args = ["day", "loop", "end"] + (["--json"] if json_output else [])
    assert cli.main(args) == 1
    assert "interrupted" in capsys.readouterr().out


def test_legacy_session_end_warns_about_unverified_loop(monkeypatch, capsys):
    from careerbrew.workday import watch

    sessions.open_session("billing", "legacy")
    sessions.write_handoff("legacy", CANARY)
    data = store.read_json(sessions.path(), {})
    data["sessions"][-1].pop("record_id")
    data["sessions"][-1].pop("project_incarnation")
    store.write_json(sessions.path(), data)
    loops = store.read_json(watch.state_path(), {})
    loops["loops"][-1].pop("session_record")
    store.write_json(watch.state_path(), loops)
    capsys.readouterr()
    assert cli.main(["work", "session", "end", "legacy"]) == 0
    output = capsys.readouterr().out
    assert "careerbrew day loop stop --id legacy" in output
    assert CANARY not in output
    assert [row["id"] for row in watch.status()["running"]] == ["legacy"]


def test_nonquiet_legacy_history_is_reported_unverified(monkeypatch, capsys):
    _prior(monkeypatch, "2026-10-27")
    data = store.read_json(sessions.path(), {})
    data["sessions"][-1].pop("project_incarnation")
    data["sessions"][-1].pop("record_id")
    store.write_json(sessions.path(), data)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["work", "session", "open", "billing", "--id", "new", "--json"]) == 0
    output = capsys.readouterr().out
    assert json.loads(output)["brief_status"] == "legacy-unverified"
    assert CANARY not in output


@pytest.mark.parametrize("identity", ["duplicate", "invalid"])
def test_invalid_session_record_identity_refuses_without_writing(identity):
    sessions.open_session("billing", "first")
    projects.add("ledger")
    data = store.read_json(sessions.path(), {})
    row = dict(data["sessions"][-1])
    row.update(id="other", project="ledger", record_id=row["record_id"] if identity == "duplicate" else "invalid")
    data["sessions"].append(row)
    store.write_json(sessions.path(), data)
    before = sessions.path().read_bytes()
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "first")
    assert sessions.path().read_bytes() == before


def test_malformed_claim_refuses_without_writing():
    sessions.open_session("billing", "first")
    data = store.read_json(sessions.path(), {})
    data["claims"].append({"unexpected": "synthetic"})
    store.write_json(sessions.path(), data)
    before = sessions.path().read_bytes()
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "first")
    assert sessions.path().read_bytes() == before


def test_partial_claim_refuses_without_writing():
    sessions.open_session("billing", "first")
    data = store.read_json(sessions.path(), {})
    data["claims"].append({"resource": "browser:sso"})
    store.write_json(sessions.path(), data)
    before = sessions.path().read_bytes()
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "first")
    assert sessions.path().read_bytes() == before


def test_partial_session_identity_refuses_without_writing():
    sessions.open_session("billing", "first")
    data = store.read_json(sessions.path(), {})
    data["sessions"][-1].pop("record_id")
    store.write_json(sessions.path(), data)
    before = sessions.path().read_bytes()
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "first")
    assert sessions.path().read_bytes() == before


@pytest.mark.parametrize("row", [{"id": "bad", "ended": "garbage"}, {"id": "bad", "project": "billing", "day": "2026-10-27", "started": "2026-10-27T09:00+00:00", "ended": "garbage"}])
def test_malformed_session_state_refuses_cli_day_end_without_writing(row, capsys):
    from careerbrew.workday import watch

    store.write_json(sessions.path(), {"sessions": [row], "claims": []})
    store.write_json(watch.state_path(), {"loops": []})
    session_before = sessions.path().read_bytes()
    loop_before = watch.state_path().read_bytes()
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) != 0
    assert sessions.path().read_bytes() == session_before
    assert watch.state_path().read_bytes() == loop_before


@pytest.mark.parametrize("day", ["2026-10-27T09:00:00", "20261027"])
def test_noncanonical_session_day_refuses_open_and_cap_bypass(day):
    config.set_value("work_session_cap", 1)
    sessions.open_session("billing", "first")
    projects.add("ledger")
    data = store.read_json(sessions.path(), {})
    data["sessions"][-1]["day"] = day
    store.write_json(sessions.path(), data)
    before = sessions.path().read_bytes()
    for project, ident in (("billing", "replacement"), ("ledger", "other")):
        with pytest.raises(sessions.SessionError):
            sessions.open_session(project, ident)
    assert sessions.path().read_bytes() == before


def test_malformed_loop_store_refuses_cli_end_without_writing(capsys):
    from careerbrew.workday import watch

    store.write_json(watch.state_path(), ["synthetic malformed store"])
    before = watch.state_path().read_bytes()
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) != 0
    assert watch.state_path().read_bytes() == before


@pytest.mark.parametrize("row", [{"id": "x", "kind": "session"}, {"id": "x", "kind": "session", "step": "", "started": "2026-10-27T09:00+00:00", "day": "2026-10-27", "stopped": {"bad": True}}])
def test_partial_loop_refuses_cli_end_without_writing(row, capsys):
    from careerbrew.workday import watch

    store.write_json(watch.state_path(), {"loops": [row]})
    before = watch.state_path().read_bytes()
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) != 0
    assert watch.state_path().read_bytes() == before


def test_session_end_refuses_invalid_loop_store_without_changing_claims():
    from careerbrew.workday import watch

    sessions.open_session("billing", "first")
    sessions.take("browser:sso", "first")
    sessions.write_handoff("first", CANARY)
    store.write_json(watch.state_path(), ["synthetic malformed store"])
    session_before = sessions.path().read_bytes()
    loop_before = watch.state_path().read_bytes()
    with pytest.raises(sessions.SessionError):
        sessions.end("first")
    assert sessions.path().read_bytes() == session_before
    assert watch.state_path().read_bytes() == loop_before


def test_interruption_after_loop_save_keeps_one_running_loop(monkeypatch):
    from careerbrew.workday import watch

    original = watch._save
    failed = False

    def interrupted(*args, **kwargs):
        nonlocal failed
        result = original(*args, **kwargs)
        if not failed:
            failed = True
            raise OSError("synthetic post-loop interruption")
        return result

    monkeypatch.setattr(watch, "_save", interrupted)
    with pytest.raises(OSError, match="synthetic post-loop interruption"):
        sessions.open_session("billing", "resume")
    sessions.open_session("billing", "resume")
    sessions.open_session("billing", "resume")
    assert [(row["kind"], row["id"]) for row in watch.status()["running"]] == [("session", "resume")]


def test_end_cannot_stop_a_new_session_that_reuses_its_id(monkeypatch):
    from careerbrew.workday import watch

    sessions.open_session("billing", "same-id")
    sessions.write_handoff("same-id", "SYNTHETIC-HANDOFF")
    original = watch.stop
    stopping = threading.Event()
    proceed = threading.Event()
    failures = []

    def interrupted(loop_id, **kwargs):
        stopping.set()
        assert proceed.wait(5)
        return original(loop_id, **kwargs)

    def end_old():
        try:
            sessions.end("same-id")
        except Exception as exc:
            failures.append(exc)

    def open_new():
        try:
            sessions.open_session("billing", "same-id")
        except Exception as exc:
            failures.append(exc)

    monkeypatch.setattr(watch, "stop", interrupted)
    ending = threading.Thread(target=end_old)
    ending.start()
    assert stopping.wait(5)
    opening = threading.Thread(target=open_new)
    opening.start()
    proceed.set()
    ending.join(10)
    opening.join(10)
    assert not ending.is_alive() and not opening.is_alive() and not failures
    assert [row["id"] for row in sessions.listing()] == ["same-id"]
    assert [(row["kind"], row["id"]) for row in watch.status()["running"]] == [("session", "same-id")]


def test_recreated_project_does_not_inherit_quiet_receipt(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    projects.edit("billing", quiet_until="2026-10-28")
    previous = projects.records()["billing"]["quiet_generation"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    sessions.open_session("billing", "resumed")
    sessions.write_handoff("resumed", "SYNTHETIC-OLD-PROJECT")
    sessions.end("resumed")
    projects.remove("billing")
    projects.add("billing")
    projects.edit("billing", quiet_until="2026-10-29")
    assert projects.records()["billing"]["quiet_generation"] != previous
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T09:00+00:00")
    assert sessions.open_session("billing", "new-project-session")["brief"] == ""


def test_recreated_project_does_not_inherit_prior_day_handoff(monkeypatch):
    _prior(monkeypatch, "2026-10-27")
    projects.remove("billing")
    projects.add("billing")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    assert sessions.open_session("billing", "new-project-session")["brief"] == ""


def test_project_incarnation_survives_archive_and_changes_on_recreation():
    original = projects.records()["billing"]["incarnation_id"]
    projects.archive("billing")
    projects.archive("billing", on=False)
    assert projects.records()["billing"]["incarnation_id"] == original
    projects.remove("billing")
    projects.add("billing")
    assert projects.records()["billing"]["incarnation_id"] != original


def test_old_incarnation_live_session_is_not_reused_or_released(monkeypatch):
    config.set_value("work_session_cap", 1)
    sessions.open_session("billing", "old-live")
    sessions.take("browser:sso", "old-live")
    projects.remove("billing")
    projects.add("billing")
    for ident in ("old-live", "new-id"):
        with pytest.raises(sessions.SessionError, match="unverified|older"):
            sessions.open_session("billing", ident)
    assert sessions.holder("browser:sso")["session"] == "old-live"
    projects.add("ledger")
    with pytest.raises(sessions.SessionError, match="sessions are live"):
        sessions.open_session("ledger", "other")


def test_legacy_live_session_is_not_reused_or_released(monkeypatch, capsys):
    config.set_value("work_session_cap", 1)
    sessions.open_session("billing", "legacy-live")
    sessions.take("browser:sso", "legacy-live")
    project_data = store.read_yaml(projects.path(), {})
    project_data["projects"]["billing"].pop("incarnation_id", None)
    store.write_yaml(projects.path(), project_data)
    session_data = store.read_json(sessions.path(), {})
    session_data["sessions"][-1].pop("project_incarnation", None)
    session_data["sessions"][-1].pop("record_id", None)
    store.write_json(sessions.path(), session_data)
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["work", "session", "open", "billing", "--id", "legacy-live"]) == 2
    output = capsys.readouterr().out
    assert "unverified" in output
    assert "billing" not in output and "browser:sso" not in output and CANARY not in output
    assert sessions.holder("browser:sso")["session"] == "legacy-live"
    projects.add("ledger")
    with pytest.raises(sessions.SessionError, match="sessions are live"):
        sessions.open_session("ledger", "other")


def test_concurrent_legacy_project_migration_has_one_identity(monkeypatch):
    project_data = store.read_yaml(projects.path(), {})
    project_data["projects"]["billing"].pop("incarnation_id", None)
    store.write_yaml(projects.path(), project_data)
    barrier = threading.Barrier(2)
    outcomes = []

    def attempt(ident):
        barrier.wait(timeout=5)
        try:
            outcomes.append(sessions.open_session("billing", ident))
        except sessions.SessionError:
            outcomes.append(None)

    threads = [threading.Thread(target=attempt, args=(ident,)) for ident in ("first", "second")]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(10)
    assert all(not thread.is_alive() for thread in threads)
    assert sum(row is not None for row in outcomes) == 1
    incarnation = projects.records()["billing"]["incarnation_id"]
    saved = store.read_json(sessions.path(), {})
    assert [row["project_incarnation"] for row in saved["sessions"]] == [incarnation]


@pytest.mark.parametrize("problem", ["invalid-quiet", "collision", "invalid-loop"])
def test_refused_legacy_open_does_not_save_project_identity(monkeypatch, problem):
    from careerbrew.workday import watch

    projects.add("ledger")
    data = store.read_yaml(projects.path(), {})
    data["projects"]["billing"].pop("incarnation_id")
    if problem == "invalid-quiet":
        data["projects"]["billing"]["quiet_generation"] = "invalid"
    if problem == "collision":
        collision = data["projects"]["ledger"]["incarnation_id"]
        monkeypatch.setattr(projects, "uuid", SimpleNamespace(uuid4=lambda: SimpleNamespace(hex=collision)))
    store.write_yaml(projects.path(), data)
    if problem == "invalid-loop":
        store.write_json(watch.state_path(), ["synthetic malformed store"])
    before = projects.path().read_bytes()
    with pytest.raises((sessions.SessionError, watch.WatchError)):
        sessions.open_session("billing", "first")
    assert projects.path().read_bytes() == before
