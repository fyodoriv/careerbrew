"""Sent PM edits reconcile without new result rows or guessed recipients."""

import json
from datetime import date, datetime
from pathlib import Path

import pytest

from careerbrew import config, connectors, secrets, store
from careerbrew.day import log as daylog
from careerbrew.day import pm_learn, watch
from careerbrew.work import evidence, feed

DAY = "2026-10-21"
DRAFT = "billing: shipped export.\ntooling: fixed CI."
FIRST = "billing: shipped export. In draft"
EDITED = "billing: shipped export. In draft; shared the runbook."
STAMP = str(datetime.fromisoformat("2026-10-21T21:30:00+00:00").timestamp())


@pytest.fixture(autouse=True)
def setup(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T18:00")
    config.set_value("pm_slack", "#team-updates")
    pm_learn.remember(DAY, DRAFT, DRAFT)


def message(text, channel=None, user="U0ME", ts=STAMP):
    raw = {"ts": ts, "user": user, "channel": channel or {"id": "C0PM", "name": "team-updates"}, "text": text}
    return connectors._slack_item(raw, "U0ME")


def test_edit_only_cycle_reconciles_and_unchanged_repeat_writes_no_pm_history(monkeypatch):
    feed.ingest([message(FIRST)])
    assert pm_learn.learn_sent(DAY)
    store.write_text(daylog.path_for(date.fromisoformat(DAY)), "Existing day log")
    monkeypatch.setattr(feed, "pull", lambda: feed.ingest([message(EDITED)]))
    monkeypatch.setattr(watch, "SOURCES", {"feed": watch._src_feed})
    monkeypatch.setattr(watch, "_notify_events", lambda: None)
    monkeypatch.setattr(watch, "_telemetry", lambda: None)
    assert watch.run_cycle()["added"] == 0
    rows = store.read_jsonl(pm_learn.path())
    assert len(rows) == 1 and rows[0]["body"] == EDITED
    assert rows[0]["edit_rules"]
    assert pm_learn.rules()["cut"] == [], "two same-day versions are one edited day"
    before = pm_learn.path().stat().st_mtime_ns
    assert watch.run_cycle()["added"] == 0
    assert pm_learn.path().stat().st_mtime_ns == before


@pytest.mark.parametrize(
    "channel,user,expected",
    [
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": True}, "U0ME", FIRST),
        ({"id": "D0PAT", "name": "U0PAT", "is_im": True}, "U0ME", None),
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": True}, "U0ALEX", None),
        ({"id": "G0TEAM", "name": "U0ALEX", "is_im": True, "is_mpim": True}, "U0ME", None),
        ({"id": "D0ALEX", "is_im": True}, "U0ME", None),
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": "false"}, "U0ME", None),
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": 1}, "U0ME", None),
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": True, "is_mpim": "false"}, "U0ME", None),
        ({"id": "D0ALEX", "name": "U0ALEX", "is_im": True, "is_mpim": ""}, "U0ME", None),
    ],
)
def test_dm_requires_exact_existing_recipient_metadata(channel, user, expected):
    config.set_value("pm_slack", "U0ALEX")
    feed.ingest([message(FIRST, channel, user)])
    assert pm_learn.pm_sent_text(DAY) == expected


def test_existing_slack_fixture_preserves_an_own_dm_recipient_through_ingestion():
    fixture = Path(__file__).resolve().parents[1] / "fixtures/http/feed/slack.com/api_search.messages.json"
    matches = json.loads(fixture.read_text())["messages"]["matches"]
    raw = next(row for row in matches if row.get("user") == "U0ME" and row["channel"].get("name") == "U0ALEX")
    config.set_value("pm_slack", raw["channel"]["name"])
    pm_learn.remember(DAY, raw["text"], raw["text"])
    feed.ingest([connectors._slack_item(raw, "U0ME")])
    assert pm_learn.pm_sent_text(DAY) == raw["text"]


def send(monkeypatch, response=None):
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "slack", "token": "env:SYNTHETIC"}])
    monkeypatch.setattr(secrets, "resolve", lambda ref: "synthetic")

    def post(url, headers, body=None):
        assert url == "https://slack.com/api/chat.postMessage"
        return response if response is not None else {"ok": True, "channel": "C0PM", "ts": STAMP}

    monkeypatch.setattr(connectors, "http_json", post)
    daylog.send_pm(FIRST)


def test_app_send_receipt_follows_only_its_message_and_keeps_latest_fallback(monkeypatch):
    send(monkeypatch)
    feed.ingest([message(EDITED, ts=str(float(STAMP) + 1))])
    assert pm_learn.pm_sent_text(DAY) == FIRST
    feed.ingest([message(EDITED)])
    assert pm_learn.learn_sent(DAY)
    assert store.read_jsonl(pm_learn.path())[0]["body"] == EDITED
    assert store.read_text(pm_learn.sent_path(DAY)).strip() == EDITED
    store.write_jsonl(feed.originals_path(), [])
    assert pm_learn.pm_sent_text(DAY) == EDITED, "a pruned source must not restore the original send"


def test_manual_record_remains_authoritative_after_an_app_send(monkeypatch):
    send(monkeypatch)
    manual = "billing: owner recorded a separate final update."
    pm_learn.save_sent(DAY, manual)
    feed.ingest([message(EDITED)])
    assert pm_learn.pm_sent_text(DAY) == manual


def test_app_send_without_response_identity_keeps_recorded_text(monkeypatch):
    send(monkeypatch, {"ok": True})
    feed.ingest([message(EDITED)])
    assert pm_learn.pm_sent_text(DAY) == FIRST


def test_receipt_stays_encrypted_and_the_data_cli_hides_its_identity(monkeypatch, capsys):
    from careerbrew import cli

    send(monkeypatch)
    receipt = pm_learn.receipt_path(DAY)
    assert store.read_json(receipt)["id"] == f"slack:C0PM:{STAMP}"
    assert b"C0PM" not in receipt.read_bytes()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["data", "show", f"day/{receipt.name}"]) == 3
    output = capsys.readouterr().out
    assert "C0PM" not in output and STAMP not in output and FIRST not in output


@pytest.mark.parametrize("receipt", [["not-a-receipt"], "not-a-receipt", {"id": 17}, {"id": ["slack:C0PM:1.1"]}, {"id": "untrusted-id"}])
def test_malformed_receipt_preserves_the_recorded_source_and_file(receipt, monkeypatch):
    seen = []
    monkeypatch.setattr(pm_learn.events, "emit", lambda name, **fields: seen.append((name, fields)))
    pm_learn.save_sent(DAY, FIRST)
    store.write_json(pm_learn.receipt_path(DAY), receipt)
    before = pm_learn.receipt_path(DAY).read_bytes()
    raw = message(EDITED)
    raw["id"] = "untrusted-id"
    feed.ingest([raw])
    assert pm_learn.pm_sent_text(DAY) == FIRST
    pm_learn.learn_sent(DAY)
    assert pm_learn.receipt_path(DAY).read_bytes() == before
    assert store.read_text(pm_learn.sent_path(DAY)).strip() == FIRST
    assert ("day.source.error", {"source_kind": "pm-receipt", "error": "InvalidReceipt", "story": "us-59"}) in seen


def test_same_day_edits_do_not_lower_the_two_distinct_day_threshold():
    feed.ingest([message(FIRST)])
    pm_learn.learn_sent(DAY)
    feed.ingest([message(EDITED)])
    pm_learn.learn_sent(DAY)
    assert pm_learn.rules()["cut"] == []
    other = "2026-10-20"
    pm_learn.remember(other, DRAFT, DRAFT)
    pm_learn.save_sent(other, FIRST)
    pm_learn.learn_sent(other)
    assert pm_learn.rules()["cut"] == ["tooling"]


def test_unchanged_sent_draft_does_not_rewrite_history():
    feed.ingest([message(DRAFT)])
    pm_learn.learn_sent(DAY)
    before = pm_learn.path().stat().st_mtime_ns if pm_learn.path().exists() else None
    pm_learn.learn_sent(DAY)
    after = pm_learn.path().stat().st_mtime_ns if pm_learn.path().exists() else None
    assert after == before


def test_edit_of_retained_older_message_keeps_its_original_sent_day(monkeypatch):
    feed.ingest([message(FIRST)])
    pm_learn.learn_sent(DAY)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T18:00")
    store.write_text(daylog.path_for(date(2026, 10, 24)), "Existing day log")
    monkeypatch.setattr(feed, "pull", lambda: feed.ingest([message(EDITED)]))
    monkeypatch.setattr(watch, "SOURCES", {"feed": watch._src_feed})
    monkeypatch.setattr(watch, "_notify_events", lambda: None)
    monkeypatch.setattr(watch, "_telemetry", lambda: None)
    assert watch.run_cycle()["added"] == 0
    rows = store.read_jsonl(pm_learn.path())
    assert [(row["day"], row["body"]) for row in rows] == [(DAY, EDITED)]


@pytest.fixture
def sent_pair_workers():
    import fcntl
    import multiprocessing
    import traceback
    from types import SimpleNamespace

    context = multiprocessing.get_context("fork")
    workers = []
    releases = []
    timeout = 30

    def event():
        gate = context.Event()
        releases.append(gate)
        return gate

    def wait(gate):
        assert gate.wait(timeout), "registered store boundary or worker attempt was not reached"

    def role(name):
        return multiprocessing.current_process().name == f"sent-pair-scope-{name}"

    def start(name, operation):
        reader, writer = context.Pipe(duplex=False)

        def invoke():
            reader.close()
            try:
                writer.send(("ok", operation()))
            except BaseException:  # report child failures to the owning test before cleanup
                writer.send(("error", traceback.format_exc(limit=8)))
            finally:
                writer.close()

        process = context.Process(target=invoke, name=f"sent-pair-scope-{name}", daemon=True)
        worker = (process, reader, writer)
        workers.append(worker)
        try:
            process.start()
        finally:
            writer.close()
        return worker

    def finish(worker):
        process, reader, _ = worker
        process.join(timeout)
        assert not process.is_alive(), f"{process.name} exceeded its bounded join"
        assert process.exitcode == 0, f"{process.name} exited {process.exitcode}"
        assert reader.poll(), f"{process.name} returned no result"
        status, value = reader.recv()
        assert status == "ok", value
        return value

    def lock_held():
        canonical = date.fromisoformat(DAY).isoformat()
        lock = pm_learn.sent_path(DAY).with_name(f".pm-sent-{canonical}.lock")
        lock.parent.mkdir(parents=True, exist_ok=True)
        with lock.open("a") as handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return True
            else:
                fcntl.flock(handle, fcntl.LOCK_UN)
                return False

    try:
        yield SimpleNamespace(event=event, wait=wait, role=role, start=start, finish=finish, lock_held=lock_held)
    finally:
        for gate in releases:
            gate.set()
        leaked = []
        for process, reader, writer in workers:
            try:
                if process.pid is not None:
                    process.join(5)
                    if process.is_alive():
                        process.terminate()
                        process.join(5)
                    if process.is_alive():
                        process.kill()
                        process.join(5)
                    if process.is_alive():
                        leaked.append(process.name)
                    else:
                        process.close()
            finally:
                reader.close()
                writer.close()
        assert not leaked, f"owned workers survived cleanup: {leaked}"


def test_sent_pair_scope_manual_record_survives_an_old_receipt_learner(monkeypatch, sent_pair_workers):
    pair = sent_pair_workers
    manual = "billing: owner final manual update."
    pm_learn.save_sent(DAY, FIRST, receipt={"channel": "C0PM", "ts": STAMP})
    feed.ingest([message(EDITED)])
    written, release, attempted = pair.event(), pair.event(), pair.event()
    write = store.write_text_if_changed

    def pause_manual(path, text):
        changed = write(path, text)
        if pair.role("manual") and path == pm_learn.sent_path(DAY) and text.strip() == manual:
            written.set()
            pair.wait(release)
        return changed

    monkeypatch.setattr(store, "write_text_if_changed", pause_manual)
    owner = pair.start("manual", lambda: pm_learn.save_sent(DAY, manual))
    pair.wait(written)

    def reconcile():
        attempted.set()
        return pm_learn.learn_sent(DAY)

    learner = pair.start("learner", reconcile)
    pair.wait(attempted)
    if pair.lock_held():
        release.set()
        pair.finish(owner)
        pair.finish(learner)
    else:
        pair.finish(learner)
        release.set()
        pair.finish(owner)
    assert store.read_text(pm_learn.sent_path(DAY)).strip() == manual
    assert not pm_learn.receipt_path(DAY).exists()


def test_sent_pair_scope_reader_returns_a_complete_before_or_after_text(monkeypatch, sent_pair_workers):
    pair = sent_pair_workers
    manual = "billing: owner final manual update."
    pm_learn.save_sent(DAY, FIRST, receipt={"channel": "C0PM", "ts": STAMP})
    feed.ingest([message(EDITED)])
    assert FIRST != EDITED != manual and FIRST != manual
    assert pm_learn.pm_sent_text(DAY) == EDITED
    captured, release, attempted = pair.event(), pair.event(), pair.event()
    read = store.read_text

    def pause_reader(path, *args, **kwargs):
        text = read(path, *args, **kwargs)
        if pair.role("reader") and path == pm_learn.sent_path(DAY):
            captured.set()
            pair.wait(release)
        return text

    monkeypatch.setattr(store, "read_text", pause_reader)
    reader = pair.start("reader", lambda: pm_learn.pm_sent_text(DAY))
    pair.wait(captured)

    def save_manual():
        attempted.set()
        pm_learn.save_sent(DAY, manual)

    owner = pair.start("manual", save_manual)
    pair.wait(attempted)
    if pair.lock_held():
        release.set()
        observed = pair.finish(reader)
        pair.finish(owner)
    else:
        pair.finish(owner)
        release.set()
        observed = pair.finish(reader)
    assert observed in {EDITED, manual}, "stale fallback is not a complete before or after read"
    assert store.read_text(pm_learn.sent_path(DAY)).strip() == manual
    assert not pm_learn.receipt_path(DAY).exists()


def test_sent_pair_scope_writers_leave_one_complete_source_receipt_pair(monkeypatch, sent_pair_workers):
    pair = sent_pair_workers
    manual = "billing: owner final manual update."
    app = "billing: second app update."
    stamp_b = "1792618210.000001"
    receipt_b = {"id": f"slack:C0PM:{stamp_b}"}
    pm_learn.save_sent(DAY, FIRST, receipt={"channel": "C0PM", "ts": STAMP})
    written, release, attempted = pair.event(), pair.event(), pair.event()
    write = store.write_text_if_changed

    def pause_app(path, text):
        changed = write(path, text)
        if pair.role("app") and path == pm_learn.sent_path(DAY) and text.strip() == app:
            written.set()
            pair.wait(release)
        return changed

    monkeypatch.setattr(store, "write_text_if_changed", pause_app)
    sender = pair.start("app", lambda: pm_learn.save_sent(DAY, app, receipt={"channel": "C0PM", "ts": stamp_b}))
    pair.wait(written)

    def save_manual():
        attempted.set()
        pm_learn.save_sent(DAY, manual)

    owner = pair.start("manual", save_manual)
    pair.wait(attempted)
    if pair.lock_held():
        release.set()
        pair.finish(sender)
        pair.finish(owner)
    else:
        pair.finish(owner)
        release.set()
        pair.finish(sender)
    saved = store.read_text(pm_learn.sent_path(DAY)).strip()
    receipt = store.read_json(pm_learn.receipt_path(DAY)) if pm_learn.receipt_path(DAY).exists() else None
    assert (saved, receipt) in [(manual, None), (app, receipt_b)], "a receipt must belong to its complete app save"
