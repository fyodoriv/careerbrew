"""Quiet projects do not produce automated work before their return date."""

import json
import threading

import pytest

from careerbrew import cli, config, notify, paths, server, store
from careerbrew.work import feed, projects, sessions
from careerbrew.workday import questions

from .conftest import FIXTURES


@pytest.fixture(autouse=True)
def project(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00:00+00:00")
    assert cli.main(["init"]) == 0
    assert cli.main(["work", "project", "add", "billing"]) == 0


def set_quiet(value):
    try:
        return cli.main(["work", "project", "set", "billing", "--quiet-until", value])
    except SystemExit as exc:
        return exc.code


def test_future_quiet_refuses_a_new_project_session():
    assert set_quiet("2026-10-23") == 0
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "quiet-session")
    assert sessions.listing() == []


def test_future_quiet_skips_its_notification_but_keeps_unrelated(tmp_path, monkeypatch):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "alerter").symlink_to(FIXTURES / "notifier" / "fake-notifier.sh")
    log = tmp_path / "notifier.log"
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_NOTIFIER_LOG", str(log))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setattr(server, "start_background", lambda port: "http://127.0.0.1:1/#token=synthetic")
    for key, value in {"notify_events": "true", "notify_backend": "alerter", "notify_quiet_hours": "none", "notify_batch_minutes": "0", "notify_min_priority": "1"}.items():
        config.set_value(key, value)
    config.set_value("project_priority", "billing=*invoice*")
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t1"},
            {"id": "slack:C2:2.0", "at": "2026-10-21T09:30:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t2"},
        ]
    )
    assert {(row["ref"], row["project_quiet"]) for row in notify.candidates()} == {("slack:C1:1.0", True), ("slack:C2:2.0", False)}
    notify.run(wait=True)
    rows = {row["refs"][0]: row for row in notify.load()["items"]}
    assert rows["slack:C1:1.0"]["popup"] == "held:project"
    assert rows["slack:C2:2.0"]["popup"] == "sent"
    assert log.read_text().count("--- alerter") == 1


def test_on_return_date_session_receives_prior_handoff(monkeypatch):
    sessions.open_session("billing", "old-session")
    sessions.write_handoff("old-session", "Resume the invoice review")
    sessions.end("old-session")
    assert set_quiet("2026-10-23") == 0
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    resumed = sessions.open_session("billing", "new-session")
    assert resumed["brief"] == "Resume the invoice review"
    assert resumed["handoff"] == ""


def test_none_clears_quiet_and_allows_session():
    assert set_quiet("2026-10-23") == 0
    assert set_quiet("none") == 0
    assert sessions.open_session("billing", "new-session")["id"] == "new-session"


def test_list_shows_quiet_date_and_rejects_invalid_change(capsys):
    assert set_quiet("2026-10-23") == 0
    assert set_quiet("2026-02-30") != 0
    assert cli.main(["work", "project", "list"]) == 0
    assert "quiet until 2026-10-23" in capsys.readouterr().out


@pytest.fixture
def notifier(tmp_path, monkeypatch):
    bin_dir = tmp_path / "notify-bin"
    bin_dir.mkdir()
    (bin_dir / "alerter").symlink_to(FIXTURES / "notifier" / "fake-notifier.sh")
    log = tmp_path / "notify.log"
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_NOTIFIER_LOG", str(log))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setattr(server, "start_background", lambda port: "http://127.0.0.1:1/#token=synthetic")
    for key, value in {"notify_events": "true", "notify_backend": "alerter", "notify_quiet_hours": "none", "notify_batch_minutes": "0", "notify_min_priority": "1", "project_priority": "billing=*invoice*"}.items():
        config.set_value(key, value)
    return log


def add_invoice_ask():
    feed.ingest([{"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t1"}])


def test_existing_open_notification_is_not_closed_by_quiet_period(notifier, monkeypatch):
    add_invoice_ask()
    notify.run(wait=True)
    assert notify.load()["items"][0]["status"] == "new"
    assert set_quiet("2026-10-23") == 0
    notify.run(wait=True)
    assert notify.load()["items"][0]["status"] == "new"
    assert notifier.read_text().count("--- alerter") == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_held_notification_delivers_once_on_return_date(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    assert not notifier.exists()
    assert notify.load()["items"][0]["status"] == "new"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    assert notify.load()["items"][0]["status"] == "new"


def test_a_session_already_running_cannot_queue_a_quiet_project_ask():
    sessions.open_session("billing", "running-session")
    assert set_quiet("2026-10-23") == 0
    with pytest.raises(sessions.SessionError):
        sessions.ask("running-session", "Need a decision?")


def test_return_brief_respects_business_policy_on_first_open_and_reuse(monkeypatch, capsys):
    sessions.open_session("billing", "old-session")
    sessions.write_handoff("old-session", "synthetic confidential handoff")
    sessions.end("old-session")
    assert set_quiet("2026-10-23") == 0
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["work", "session", "open", "billing", "--id", "new-session", "--json"]) == 0
    assert "synthetic confidential handoff" not in capsys.readouterr().out
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["work", "session", "open", "billing", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["brief"] == "synthetic confidential handoff"


def test_list_stops_calling_a_project_quiet_on_its_return_date(monkeypatch, capsys):
    assert set_quiet("2026-10-23") == 0
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    capsys.readouterr()
    assert cli.main(["work", "project", "list"]) == 0
    assert "quiet until" not in capsys.readouterr().out


def test_existing_question_waits_without_being_closed_until_return(monkeypatch):
    sessions.open_session("billing", "running-session")
    question = sessions.ask("running-session", "Need a decision?")
    assert set_quiet("2026-10-23") == 0
    assert question["id"] not in [q["id"] for q in questions.recheck()["ask"]]
    assert question["id"] in [q["id"] for q in questions.open_records()]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    assert question["id"] in [q["id"] for q in questions.recheck()["ask"]]


def test_unrelated_ask_in_shared_thread_is_not_held(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2


def test_unrelated_ask_arriving_after_quiet_hold_is_delivered(notifier):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    assert not notifier.exists()
    feed.ingest([{"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t1"}])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_held_ask_closed_before_return_does_not_notify(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    feed.ingest([{"id": "slack:C1:1.1", "at": "2026-10-21T09:50:00Z", "title": "answered", "source": "slack", "kind": "message", "actor": "me", "to_me": False, "thread": "t1"}])
    notify.run(wait=True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert not notifier.exists()


def test_owner_reading_held_ask_does_not_get_later_popup(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    notify.act(notify.load()["items"][0]["id"], "read")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert not notifier.exists()


def test_closed_held_ask_in_shared_thread_does_not_repeat_unrelated_alert(notifier):
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    feed.ingest([{"id": "slack:C1:1.1", "at": "2026-10-21T09:30:30Z", "title": "answered", "source": "slack", "kind": "message", "actor": "me", "to_me": False, "thread": "shared"}])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_notification_view_explains_project_hold(notifier):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    assert "project" in notify.view()["items"][0]["popup"]


def test_clearing_quiet_early_keeps_the_prior_handoff_as_brief():
    sessions.open_session("billing", "old-session")
    sessions.write_handoff("old-session", "Resume the invoice review")
    sessions.end("old-session")
    assert set_quiet("2026-10-23") == 0
    assert set_quiet("none") == 0
    resumed = sessions.open_session("billing", "new-session")
    assert resumed["brief"] == "Resume the invoice review"
    assert resumed["handoff"] == ""


def configure_repo_source():
    config.unset("project_priority")
    projects.add_source("billing", "repo", "example/billing")
    store.write_yaml(paths.data_root() / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "api": "https://api.github.com", "token": "env:SYNTHETIC_TOKEN"}]})


def repo_ask(repo="billing"):
    return {"id": f"github:pr:example/{repo}#12", "at": "2026-10-21T09:30:00Z", "title": "Review requested", "source": "github", "kind": "review-request", "actor": "other", "to_me": True, "url": f"https://github.com/example/{repo}/pull/12"}


def test_quiet_unique_repo_source_holds_ask_without_priority_rule(notifier):
    configure_repo_source()
    assert set_quiet("2026-10-23") == 0
    feed.ingest([repo_ask()])
    notify.run(wait=True)
    assert not notifier.exists()
    assert notify.load()["items"][0]["popup"] == "held:project"


def test_handled_shared_thread_delivers_new_unrelated_ask(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    add_invoice_ask()
    notify.run(wait=True)
    notify.act(notify.load()["items"][0]["id"], "read")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    feed.ingest([{"id": "slack:C1:2.0", "at": "2026-10-21T10:04:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t1"}])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_ambiguous_or_distinct_repo_is_not_silently_muted(notifier):
    configure_repo_source()
    assert cli.main(["work", "project", "add", "ledger"]) == 0
    projects.add_source("ledger", "repo", "example/billing")
    assert set_quiet("2026-10-23") == 0
    feed.ingest([repo_ask(), repo_ask("other")])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    assert len(notify.load()["items"]) == 2


def test_malformed_url_does_not_break_unrelated_notifications(notifier):
    configure_repo_source()
    assert set_quiet("2026-10-23") == 0
    item = repo_ask("other")
    item["url"] = "https://github.com:bad/example/other/pull/12"
    feed.ingest([item])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_quiet_save_before_delivery_holds_reserved_popup(notifier, monkeypatch):
    add_invoice_ask()
    ready = threading.Event()
    proceed = threading.Event()
    original = notify.deliver
    errors = []

    def delayed(popup, wait=False):
        ready.set()
        assert proceed.wait(5)
        original(popup, wait=wait)

    def run():
        try:
            notify.run(wait=True)
        except Exception as exc:
            errors.append(exc)

    monkeypatch.setattr(notify, "deliver", delayed)
    thread = threading.Thread(target=run)
    thread.start()
    assert ready.wait(5)
    projects.edit("billing", quiet_until="2026-10-23")
    proceed.set()
    thread.join(10)
    assert not thread.is_alive() and not errors
    assert not notifier.exists()
    assert notify.load()["items"][0]["popup"] == "held:project"


def test_quiet_save_before_detached_present_holds_reserved_popup(notifier, monkeypatch):
    add_invoice_ask()
    _, popup = notify.process()
    assert popup is not None
    projects.edit("billing", quiet_until="2026-10-23")
    notify.present(popup["id"])
    assert not notifier.exists()
    assert notify.load()["items"][0]["popup"] == "held:project"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_quiet_save_recomposes_mixed_reserved_popup(notifier):
    add_invoice_ask()
    feed.ingest([{"id": "slack:C2:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t2"}])
    _, popup = notify.process()
    assert popup is not None and len(popup["ids"]) == 2
    projects.edit("billing", quiet_until="2026-10-23")
    notify.present(popup["id"])
    assert notifier.read_text().count("--- alerter") == 1
    rows = {row["refs"][0]: row for row in notify.load()["items"]}
    assert rows["slack:C1:1.0"]["popup"] == "held:project"
    assert rows["slack:C2:2.0"]["popup"] == "sent"
    assert len(notify.load()["popups"][-1]["ids"]) == 1


def test_failed_notifier_launch_restores_reservation(notifier, monkeypatch):
    add_invoice_ask()

    def fail(*args, **kwargs):
        raise OSError("synthetic launch failure")

    monkeypatch.setattr(notify.subprocess, "Popen", fail)
    result = notify.run(wait=True)
    row = notify.load()["items"][0]
    assert row["status"] == "new" and row["popup"] is None
    assert notify.load()["popups"] == []
    assert result["reason"] != "sent"


def test_click_wait_does_not_hold_project_lock(notifier, monkeypatch):
    add_invoice_ask()
    _, popup = notify.process()
    assert popup is not None
    waiting = threading.Event()
    release = threading.Event()
    saved = threading.Event()
    errors = []

    class Child:
        def communicate(self, timeout=None):
            waiting.set()
            assert release.wait(5)
            return "", ""

    monkeypatch.setattr(notify.subprocess, "Popen", lambda *args, **kwargs: Child())

    def present():
        try:
            notify.present(popup["id"])
        except Exception as exc:
            errors.append(exc)

    presenter = threading.Thread(target=present)
    presenter.start()
    assert waiting.wait(5)

    def save():
        try:
            projects.edit("billing", quiet_until="2026-10-23")
            saved.set()
        except Exception as exc:
            errors.append(exc)

    writer = threading.Thread(target=save)
    writer.start()
    try:
        assert saved.wait(3)
    finally:
        release.set()
        writer.join(5)
        presenter.join(5)
    assert not writer.is_alive() and not presenter.is_alive() and not errors


def test_disabled_backend_after_reservation_restores_it(notifier, monkeypatch):
    add_invoice_ask()
    _, popup = notify.process()
    assert popup is not None
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    assert notify.deliver(popup, wait=True) is False
    assert notify.load()["items"][0]["popup"] is None
    assert notify.load()["popups"] == []


def test_quiet_item_source_only_holds_its_exact_item(notifier):
    config.unset("project_priority")
    projects.add_source("billing", "url", "https://github.com/example/billing/pull/12")
    assert set_quiet("2026-10-23") == 0
    other = repo_ask()
    other["id"] = "github:pr:example/billing#13"
    other["url"] = "https://github.com/example/billing/pull/13"
    feed.ingest([repo_ask(), other])
    notify.run(wait=True)
    rows = {row["refs"][0]: row for row in notify.load()["items"]}
    assert rows["github:pr:example/billing#12"]["popup"] == "held:project"
    assert rows["github:pr:example/billing#13"]["popup"] == "sent"


def test_reading_unrelated_popup_preserves_held_ask(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Open")
    monkeypatch.setattr(notify, "open_view", lambda view: True)
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2


def test_closed_reservation_does_not_close_newer_live_ask(notifier):
    feed.ingest([{"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "first", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    _, popup = notify.process()
    assert popup is not None
    feed.ingest([{"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "second", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    notify.process()
    feed.ingest([{"id": "slack:C1:1.1", "at": "2026-10-21T09:30:30Z", "title": "answered first", "source": "slack", "kind": "message", "actor": "me", "to_me": False, "thread": "shared"}])
    assert [row["id"] for row in feed.open_asks()] == ["slack:C1:2.0"]
    notify.present(popup["id"])
    assert not notifier.exists()
    assert notify.load()["items"][0]["status"] == "new"
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_unconfirmed_detached_child_is_visible_without_blind_retry(notifier, monkeypatch):
    add_invoice_ask()
    launches = []

    class DeadChild:
        def poll(self):
            return 1

    def spawn(*args, **kwargs):
        launches.append(1)
        return DeadChild()

    monkeypatch.setattr(notify.subprocess, "Popen", spawn)
    notify.run(wait=False)
    assert "not confirmed" in notify.view()["items"][0]["popup"]
    notify.run(wait=False)
    assert launches == [1]


def test_two_presenters_cannot_launch_the_same_reservation(notifier, monkeypatch):
    add_invoice_ask()
    _, popup = notify.process()
    assert popup is not None
    waiting, release, second_done = threading.Event(), threading.Event(), threading.Event()
    launches, errors = [], []

    class Child:
        def communicate(self, timeout=None):
            waiting.set()
            assert release.wait(5)
            return "", ""

    def spawn(*args, **kwargs):
        launches.append(1)
        return Child()

    monkeypatch.setattr(notify.subprocess, "Popen", spawn)

    def present(done=None):
        try:
            notify.present(popup["id"])
        except Exception as exc:
            errors.append(exc)
        finally:
            if done is not None:
                done.set()

    first = threading.Thread(target=present)
    second = threading.Thread(target=present, args=(second_done,))
    first.start()
    assert waiting.wait(5)
    second.start()
    try:
        assert second_done.wait(3)
        assert launches == [1]
    finally:
        release.set()
        first.join(5)
        second.join(5)
    assert not first.is_alive() and not second.is_alive() and not errors


def test_snoozing_unrelated_popup_does_not_consume_held_ask(notifier, monkeypatch):
    config.set_value("notify_snooze_minutes", "15")
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:20:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 3


def test_cancelled_visible_reservation_keeps_quiet_ask(notifier, monkeypatch):
    assert set_quiet("2026-10-23") == 0
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    _, popup = notify.process()
    assert popup is not None
    feed.mark("slack:C1:2.0", True)
    notify.present(popup["id"])
    assert not notifier.exists()
    assert notify.load()["items"][0]["status"] == "new"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_snoozed_ask_becoming_quiet_returns_on_its_project_date(notifier, monkeypatch):
    assert cli.main(["work", "project", "add", "ledger"]) == 0
    config.set_value("project_priority", "billing=*invoice*, ledger=*ledger*")
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "ledger work", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    projects.edit("ledger", quiet_until="2026-10-23")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:20:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-23T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T09:00:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 3


def test_two_snoozed_asks_keep_separate_deadlines(notifier, monkeypatch):
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated B", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    feed.ingest([{"id": "slack:C1:3.0", "at": "2026-10-21T10:04:00Z", "title": "unrelated C", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:2.0"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:21:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:3.0"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-24T09:00:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:1.0"]


def test_quiet_ends_at_local_midnight_for_sessions_and_notifications(notifier, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T23:30:00-04:00")
    assert set_quiet("2026-10-22") == 0
    feed.ingest([{"id": "slack:C1:1.0", "at": "2026-10-22T03:00:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "t1"}])
    assert notify.candidates()[0]["project_quiet"] is True
    with pytest.raises(sessions.SessionError):
        sessions.open_session("billing", "midnight-session")
    assert projects.list_rows()[0]["quiet"] is True
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-22T00:00:00-04:00")
    assert notify.candidates()[0]["project_quiet"] is False
    assert sessions.open_session("billing", "midnight-session")["id"] == "midnight-session"
    assert projects.list_rows()[0]["quiet"] is False


def test_same_repo_on_another_host_is_not_inferred_as_quiet(notifier):
    configure_repo_source()
    assert set_quiet("2026-10-23") == 0
    item = repo_ask()
    item["url"] = "https://github.example.com/example/billing/pull/12"
    feed.ingest([item])
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1


def test_cli_explains_when_a_notifier_did_not_launch(notifier, monkeypatch, capsys):
    add_invoice_ask()

    def fail(*args, **kwargs):
        raise OSError("synthetic launch failure")

    monkeypatch.setattr(notify.subprocess, "Popen", fail)
    capsys.readouterr()
    assert cli.main(["notify", "check", "--wait"]) == 0
    assert "not sent" in capsys.readouterr().out


def test_cli_calls_an_unconfirmed_detached_launch_scheduled(notifier, monkeypatch, capsys):
    add_invoice_ask()

    class DeadChild:
        def poll(self):
            return 1

    monkeypatch.setattr(notify.subprocess, "Popen", lambda *args, **kwargs: DeadChild())
    capsys.readouterr()
    assert cli.main(["notify", "check"]) == 0
    assert "scheduled" in capsys.readouterr().out


def test_closing_held_ask_does_not_send_snoozed_ask_early(notifier, monkeypatch):
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    feed.mark("slack:C1:1.0", True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2


def test_new_ask_is_not_delayed_by_another_ask_snooze(notifier, monkeypatch):
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated B", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    feed.mark("slack:C1:1.0", True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    notify.run(wait=True)
    feed.ingest([{"id": "slack:C1:3.0", "at": "2026-10-21T10:05:30Z", "title": "unrelated C", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:06:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 2
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:3.0"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    assert notifier.read_text().count("--- alerter") == 3
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:2.0"]


def test_two_snoozes_stay_independent_after_quiet_ask_closes(notifier, monkeypatch):
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated B", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    feed.mark("slack:C1:1.0", True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    notify.run(wait=True)
    feed.ingest([{"id": "slack:C1:3.0", "at": "2026-10-21T10:05:30Z", "title": "unrelated C", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:06:00+00:00")
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:2.0"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:22:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:3.0"]


def test_reading_new_ask_does_not_consume_another_snooze(notifier, monkeypatch):
    assert set_quiet("2026-10-24") == 0
    config.set_value("notify_snooze_minutes", "15")
    feed.ingest(
        [
            {"id": "slack:C1:1.0", "at": "2026-10-21T09:30:00Z", "title": "invoice export", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
            {"id": "slack:C1:2.0", "at": "2026-10-21T09:31:00Z", "title": "unrelated B", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"},
        ]
    )
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    feed.mark("slack:C1:1.0", True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:05:00+00:00")
    notify.run(wait=True)
    feed.ingest([{"id": "slack:C1:3.0", "at": "2026-10-21T10:05:30Z", "title": "unrelated C", "source": "slack", "kind": "dm", "actor": "other", "to_me": True, "thread": "shared"}])
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:06:00+00:00")
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Open")
    monkeypatch.setattr(notify, "open_view", lambda view: True)
    notify.run(wait=True)
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:16:00+00:00")
    notify.run(wait=True)
    assert [ref for refs in notify.load()["popups"][-1]["refs"].values() for ref in refs] == ["slack:C1:2.0"]
