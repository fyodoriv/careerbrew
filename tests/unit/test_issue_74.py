"""Issue #74: opt-in notifications about important new events that are useful and actionable.

The fake notifier (tests/fixtures/notifier/fake-notifier.sh) stands in for alerter and
terminal-notifier: it logs every call and can simulate a click. PATH holds only it and the
system folders, and `notify_backend` names it, so no test ever shows a real pop-up.
"""

from __future__ import annotations

import json
import shlex
import sys
from types import SimpleNamespace

import pytest

from careerbrew import config, notify, plugins, server
from careerbrew.cli import main
from careerbrew.day import inbox, watch
from careerbrew.work import feed, priorities

from .conftest import CANARIES, FIXTURES

NOW = "2026-10-21T10:00"  # Etc/GMT+4: 14:00Z


def ev(eid: str, kind: str, at: str = "2026-10-21T13:30:00Z", **kw):
    base = {"id": eid, "at": at, "title": f"title of {eid}", "source": "slack", "kind": kind, "actor": "other", "to_me": True, "thread": f"t-{eid}"}
    return {**base, **kw}


@pytest.fixture
def fake(tmp_path, monkeypatch):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "notifier.log"
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_NOTIFIER_LOG", str(log))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    monkeypatch.setattr(watch, "notify", lambda text: pytest.fail("the osascript fallback must not run here"))

    def use(tool: str = "alerter", **settings):
        (bin_dir / tool).symlink_to(FIXTURES / "notifier" / "fake-notifier.sh")
        for key, value in {"notify_events": "true", "notify_backend": tool, "notify_quiet_hours": "none", "notify_batch_minutes": "0", **settings}.items():
            config.set_value(key, str(value))

    def calls() -> list[list[str]]:
        out: list[list[str]] = []
        for line in log.read_text().splitlines() if log.exists() else []:
            if line.startswith("--- "):
                out.append([line[4:]])
            else:
                out[-1].append(line)
        return out

    def arg(call: list[str], flag: str) -> str:
        return call[call.index(flag) + 1]

    return SimpleNamespace(use=use, calls=calls, arg=arg, log=log)


def at(monkeypatch, when: str):
    monkeypatch.setenv("CAREERBREW_NOW", when)


# ---------- opt-in and config ----------


def test_off_by_default_nothing_is_stored_or_shown(fake):
    feed.ingest([ev("slack:C1:1.0", "dm")])
    r = notify.run()
    assert r["enabled"] is False and not notify.path().exists() and fake.calls() == []


def test_config_validates_every_notification_setting():
    for key, bad, fix in (
        ("notify_quiet_hours", "25:00-08:00", "HH:MM-HH:MM"),
        ("notify_quiet_hours", "08:00-08:00", "different times"),
        ("notify_quiet_hours", "evenings", "HH:MM-HH:MM"),
        ("notify_min_priority", "6", "from 1 to 5"),
        ("notify_daily_cap", "-1", "from 0 to 50"),
        ("notify_backend", "growl", "auto, alerter, terminal-notifier, osascript"),
        ("notify_events", "maybe", "true or false"),
    ):
        with pytest.raises(ValueError, match=fix):
            config.set_value(key, bad)
    assert config.set_value("notify_quiet_hours", "none") == ""
    assert config.set_value("notify_quiet_hours", "22:30-07:00") == "22:30-07:00"
    keys = {r["key"] for r in config.effective()}
    assert {"notify_events", "notify_min_priority", "notify_quiet_hours", "notify_daily_cap", "notify_batch_minutes", "notify_snooze_minutes", "notify_backend"} <= keys


# ---------- priority ----------


def test_priority_by_kind_with_a_bonus_for_the_rank_1_project(fake):
    fake.use(notify_min_priority="1")
    config.set_value("project_priority", "billing=*invoice*")
    feed.ingest(
        [
            ev("pagerduty:P1", "page", source="pagerduty", status="triggered"),
            ev("pagerduty:P2", "page", source="pagerduty", status="acknowledged"),
            ev("slack:C1:1.0", "dm"),
            ev("slack:C2:2.0", "mention"),
            ev("slack:C3:3.0", "mention", title="Mention about the invoice export"),
            ev("github:pr-9", "review-request", source="github"),
            ev("outlook:mail:1", "mail", source="outlook", importance="high"),
            ev("slack:C4:4.0", "message", actor="bot"),
        ]
    )
    got = {c["ref"]: c["priority"] for c in notify.candidates(notify.calendar.now())}
    assert got == {"pagerduty:P1": 5, "pagerduty:P2": 3, "slack:C1:1.0": 4, "slack:C2:2.0": 3, "slack:C3:3.0": 4, "github:pr-9": 4, "outlook:mail:1": 3}, "the bot message is noise, never a notification"
    top = next(c for c in notify.candidates() if c["ref"] == "slack:C3:3.0")
    assert top["why"].endswith("; it is on your rank-1 project") and "billing" not in top["why"], "the why goes to pop-ups: no project name"
    assert "+1 for billing, your rank-1 project" in top["priority_why"], "the name shows only in the local web app"


def test_threshold_keeps_lower_priorities_out(fake):
    fake.use(notify_min_priority="4")
    feed.ingest([ev("slack:C1:1.0", "dm"), ev("slack:C2:2.0", "mention"), ev("pagerduty:P1", "page", source="pagerduty", status="triggered")])
    notify.run(wait=True)
    assert sorted(r["kind"] for r in notify.load()["items"]) == ["dm", "page"]


def test_an_old_event_is_not_news_and_a_personal_one_never_notifies(fake):
    fake.use(notify_min_priority="1")
    feed.ingest([ev("slack:C1:1.0", "dm", at="2026-10-19T13:30:00Z"), ev("slack:C2:2.0", "dm", personal=True)])
    notify.run(wait=True)
    assert notify.load()["items"] == [] and fake.calls() == []


# ---------- useful: what, why, next; no content in the pop-up ----------


def test_each_popup_says_what_why_and_next_with_no_titles_names_or_token(fake, home, monkeypatch):
    fake.use()
    (home / "sensitive-terms.txt").write_text("PROJECTZEBRA\n")
    feed.ingest([ev("slack:C1:1.0", "dm", title=f"DM from {CANARIES[0]} about PROJECTZEBRA", channel=CANARIES[1], **{"from": CANARIES[0]})])
    monkeypatch.setattr(server, "start_background", lambda port: "http://127.0.0.1:1/#token=SECRET-TOKEN")
    notify.run(wait=True)
    (call,) = fake.calls()
    assert call[0] == "alerter"
    assert fake.arg(call, "-subtitle") == "Slack: a direct message asks you"
    assert fake.arg(call, "-message") == "Why: it was sent to you directly and you have not replied. Next: reply in Slack."
    blob = fake.log.read_text()
    for secret in [*CANARIES, "title of", "SECRET-TOKEN", "token="]:
        assert secret not in blob
    (row,) = notify.view()["items"]
    assert row["title"] == f"DM from {CANARIES[0]} about PROJECTZEBRA", "the owner's web app shows the title"
    assert CANARIES[0] not in notify.path().read_bytes().decode("latin-1"), "the list is encrypted at rest"


def test_a_popup_never_names_the_rank_1_project_or_the_plugin(fake, acme_plugin, monkeypatch, capsys):
    plugins.add(str(acme_plugin))
    fake.use(notify_min_priority="3")
    config.set_value("project_priority", "nightjar=*invoice*")  # a codename that is not in sensitive-terms.txt
    feed.ingest([ev("slack:C3:3.0", "mention", title="Mention about the invoice export")])
    notify.run(wait=True)
    at(monkeypatch, "2026-10-21T10:30")
    inbox.suggest("acme", "Review the synthetic roadmap doc", "the roadmap doc changed", "Plans time to review it")
    notify.run(wait=True)
    first, second = fake.calls()
    assert fake.arg(first, "-message") == "Why: a mention asks you and you have not replied in the thread; it is on your rank-1 project. Next: reply in the thread in Slack."
    assert fake.arg(second, "-subtitle") == "careerbrew: a plugin suggests something"
    blob = fake.log.read_text().lower()
    assert "nightjar" not in blob and "acme" not in blob, "a pop-up names kinds, sources, and counts only"
    assert main(["notify", "list"]) == 0
    out = capsys.readouterr().out.lower()
    assert "nightjar" not in out and "acme" not in out, "nor does the list an agent can read"
    rows = {r["kind"]: r for r in notify.view()["items"]}
    assert "nightjar, your rank-1 project" in rows["mention"]["priority_why"] and "plugin acme" in rows["suggestion"]["title"], "the local web app still names them"


# ---------- de-dupe and batching ----------


def test_repeats_in_one_thread_are_one_notification_and_never_pop_twice(fake):
    fake.use(notify_min_priority="3")
    feed.ingest([ev("slack:C2:1.0", "mention", thread="slack:C2:1.0"), ev("slack:C2:1.5", "mention", at="2026-10-21T13:40:00Z", thread="slack:C2:1.0")])
    notify.run(wait=True)
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["count"] == 2 and len(fake.calls()) == 1


def test_a_burst_is_one_popup_and_more_within_the_window_wait_for_the_next(fake, monkeypatch):
    fake.use(notify_batch_minutes="10")
    feed.ingest([ev("pagerduty:P1", "page", source="pagerduty", status="triggered"), ev("slack:C1:1.0", "dm"), ev("github:pr-1", "review-request", source="github")])
    notify.run(wait=True)
    (call,) = fake.calls()
    assert fake.arg(call, "-subtitle") == "3 new for you: 1 page, 1 direct message, 1 review request"
    assert fake.arg(call, "-message") == "Most urgent: PagerDuty: a page is assigned to you. Next: open careerbrew to act on each one."
    assert fake.arg(call, "-actions") == "Open,Snooze"
    at(monkeypatch, "2026-10-21T10:05")
    feed.ingest([ev("slack:C5:5.0", "dm", at="2026-10-21T14:04:00Z"), ev("slack:C6:6.0", "dm", at="2026-10-21T14:04:30Z")])
    assert notify.run(wait=True)["reason"] == "batch" and len(fake.calls()) == 1
    at(monkeypatch, "2026-10-21T10:11")
    notify.run(wait=True)
    assert len(fake.calls()) == 2 and fake.arg(fake.calls()[1], "-subtitle") == "2 new for you: 2 direct messages"


# ---------- quiet hours (pinned clock) and the daily cap ----------


def test_quiet_hours_wrap_past_midnight():
    from datetime import datetime

    def t(hhmm: str) -> datetime:
        return datetime.fromisoformat(f"2026-10-21T{hhmm}")

    assert notify.in_quiet(t("22:00"), "19:00-08:00") and notify.in_quiet(t("07:59"), "19:00-08:00")
    assert not notify.in_quiet(t("08:00"), "19:00-08:00") and not notify.in_quiet(t("18:59"), "19:00-08:00")
    assert notify.in_quiet(t("12:30"), "12:00-13:00") and not notify.in_quiet(t("13:00"), "12:00-13:00")
    assert not notify.in_quiet(t("03:00"), "")


def test_quiet_hours_hold_then_one_while_you_were_away_popup(fake, monkeypatch):
    fake.use(notify_quiet_hours="19:00-08:00")
    at(monkeypatch, "2026-10-21T22:00")
    feed.ingest([ev("slack:C1:1.0", "dm", at="2026-10-22T01:30:00Z"), ev("pagerduty:P1", "page", source="pagerduty", status="triggered", at="2026-10-22T01:45:00Z")])
    assert notify.run(wait=True)["reason"] == "quiet" and fake.calls() == []
    assert {r["popup"] for r in notify.load()["items"]} == {"held:quiet"}
    at(monkeypatch, "2026-10-22T08:30")
    notify.run(wait=True)
    (call,) = fake.calls()
    assert fake.arg(call, "-subtitle") == "While you were away: 1 page, 1 direct message"


def test_daily_cap_keeps_the_rest_in_the_list_and_resets_the_next_day(fake, monkeypatch):
    fake.use(notify_daily_cap="1")
    feed.ingest([ev("slack:C1:1.0", "dm")])
    notify.run(wait=True)
    at(monkeypatch, "2026-10-21T11:00")
    feed.ingest([ev("slack:C2:2.0", "dm", at="2026-10-21T14:50:00Z")])
    assert notify.run(wait=True)["reason"] == "cap"
    assert len(fake.calls()) == 1 and len(notify.view()["items"]) == 2, "past the cap it waits in the web app"
    at(monkeypatch, "2026-10-22T10:00")
    feed.ingest([ev("slack:C3:3.0", "dm", at="2026-10-22T13:50:00Z")])
    notify.run(wait=True)
    assert len(fake.calls()) == 2 and fake.arg(fake.calls()[1], "-subtitle") == "Slack: a direct message asks you", "a capped one never pops later"


# ---------- interactive: clicks and actions ----------


def _agent_task(title: str = "Review the rollout checklist") -> str:
    t = priorities.add(title, 3, 3, 1.0, [], source="agent", why="the release is Thursday")
    inbox.offer_task(t)
    return t["id"]


def test_alerter_accept_click_decides_the_suggestion_in_the_inbox(fake, monkeypatch):
    fake.use(notify_min_priority="3")
    task = _agent_task()
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Accept")
    notify.run(wait=True)
    (call,) = fake.calls()
    assert fake.arg(call, "-subtitle") == "careerbrew: an agent proposes a task" and fake.arg(call, "-actions") == "Accept,Dismiss,Snooze"
    assert next(t for t in priorities.load() if t["id"] == task)["status"] == "open", "Accept planned the task"
    assert all(i["status"] == "accepted" for i in inbox.load() if i["kind"] == "agent-task")
    (item,) = notify.load()["items"]
    assert item["status"] == "done" and item["resolved"] == "accepted"


def test_a_late_accept_or_dismiss_on_a_suggestion_decided_elsewhere_changes_nothing(fake, home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    fake.use(notify_min_priority="3")
    task = _agent_task()
    notify.run(wait=True)  # the pop-up shows; alerter waits up to an hour for a click
    (n,) = notify.load()["items"]
    inbox.dismiss(n["item"])  # meanwhile the owner dismisses it in Today
    assert notify._apply_click(notify.load()["popups"][-1], "Accept", "alerter") == "", "a late Accept click does nothing"
    assert next(t for t in priorities.load() if t["id"] == task)["status"] == "dismissed", "no task is planned"
    (n,) = notify.load()["items"]
    assert n["status"] == "done" and n["resolved"] == "decided", "never reported as accepted"
    acts = [json.loads(x) for x in (home / "logs" / "events.jsonl").read_text().splitlines()]
    assert not [r for r in acts if r["event"] == "notify.act" and r["status"] in ("accept", "dismiss")], "a late click is not counted as acted on"
    at(monkeypatch, "2026-10-21T10:30")
    _agent_task("Update the runbook")
    notify.run(wait=True)
    n = next(r for r in notify.load()["items"] if r["status"] == "new")
    inbox.dismiss(n["item"])
    capsys.readouterr()
    assert main(["notify", "act", n["id"], "accept"]) == 2, "the CLI says so too"
    out = capsys.readouterr().out
    assert "already decided in the inbox" in out and "decided in the inbox too" not in out


def test_alerter_snooze_hides_it_until_the_snooze_ends(fake, monkeypatch):
    fake.use(notify_snooze_minutes="60")
    feed.ingest([ev("slack:C1:1.0", "dm")])
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Snooze")
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["status"] == "snoozed" and item["snooze_until"].startswith("2026-10-21T11:00")
    monkeypatch.delenv("FAKE_NOTIFIER_CLICK")
    at(monkeypatch, "2026-10-21T10:30")
    notify.run(wait=True)
    assert len(fake.calls()) == 1
    at(monkeypatch, "2026-10-21T11:01")
    notify.run(wait=True)
    assert len(fake.calls()) == 2 and notify.load()["items"][0]["status"] == "new"


def test_a_batch_popup_offers_open_and_snooze_only(fake, monkeypatch):
    fake.use(notify_min_priority="3")
    _agent_task()
    feed.ingest([ev("slack:C1:1.0", "dm")])
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "Accept")
    notify.run(wait=True)
    assert {r["status"] for r in notify.load()["items"]} == {"new"}, "Accept is not a batch action: nothing changes"
    assert all(i["status"] == "open" for i in inbox.load() if i["kind"] == "agent-task")
    notify._apply_click(notify.load()["popups"][-1], "Snooze", "alerter")
    assert {r["status"] for r in notify.load()["items"]} == {"snoozed"}, "Snooze on a batch snoozes each one"


def test_the_list_shows_a_suggestion_decided_in_today_as_closed_at_once(fake):
    fake.use(notify_min_priority="3")
    _agent_task()
    notify.run(wait=True)
    inbox.dismiss(notify.load()["items"][0]["item"])
    (row,) = notify.view()["items"]
    assert row["status"] == "done" and row["resolved"] == "decided" and row["actions"] == []


def test_alerter_open_click_opens_the_matching_view_through_the_open_flow(fake, monkeypatch):
    fake.use()
    feed.ingest([ev("slack:C1:1.0", "dm")])
    opened: list[str] = []
    monkeypatch.setattr(server, "start_background", lambda port: f"http://127.0.0.1:{port}/#token=SECRET-TOKEN")
    monkeypatch.setattr("webbrowser.open", lambda url: opened.append(url) or True)
    monkeypatch.setenv("FAKE_NOTIFIER_CLICK", "@CONTENTCLICKED")
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert opened == [f"http://127.0.0.1:8765/#/notifications/{item['id']}&token=SECRET-TOKEN"] and item["status"] == "read"
    assert "SECRET-TOKEN" not in fake.log.read_text()


def test_terminal_notifier_click_runs_notify_open_without_the_token(fake, monkeypatch):
    fake.use("terminal-notifier")
    feed.ingest([ev("slack:C1:1.0", "dm")])
    notify.run()
    (call,) = fake.calls()
    (item,) = notify.load()["items"]
    command = shlex.split(fake.arg(call, "-execute"))
    assert command[-4:] == ["careerbrew", "notify", "open", item["id"]] and sys.executable in command
    assert "token" not in fake.log.read_text().lower()
    opened: list[str] = []
    monkeypatch.setattr(server, "start_background", lambda port: f"http://127.0.0.1:{port}/#token=SECRET-TOKEN")
    monkeypatch.setattr("webbrowser.open", lambda url: opened.append(url) or True)
    assert main(command[command.index("careerbrew") + 1 :]) == 0, "the click runs this command"
    assert opened[0].startswith("http://127.0.0.1:8765/#/notifications/n-") and notify.load()["items"][0]["status"] == "read"


def test_osascript_is_the_fallback_without_a_clickable_notifier(fake, monkeypatch):
    sent: list[str] = []
    monkeypatch.setattr(watch, "notify", lambda text: sent.append(text))
    for key, value in {"notify_events": "true", "notify_quiet_hours": "none"}.items():
        config.set_value(key, value)
    monkeypatch.setattr(notify.sys, "platform", "darwin")
    monkeypatch.setattr(notify.shutil, "which", lambda name: "/usr/bin/osascript" if name == "osascript" else None)
    assert notify.backend()[0] == "osascript"
    feed.ingest([ev("slack:C1:1.0", "dm")])
    notify.run()
    assert sent == ["Slack: a direct message asks you. Why: it was sent to you directly and you have not replied. Next: reply in Slack."]


def test_explicit_backend_that_is_missing_shows_nothing_and_names_the_fix(fake, monkeypatch):
    config.set_value("notify_backend", "alerter")
    kind, _exe, note = notify.backend()
    assert kind == "none" and "brew install vjeantet/tap/alerter" in note
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    assert notify.backend()[0] == "none"


def test_an_answered_ask_closes_and_a_new_ask_in_that_thread_reopens(fake, monkeypatch):
    fake.use()
    feed.ingest([ev("slack:C1:1.0", "dm", thread="slack:C1")])
    notify.run(wait=True)
    feed.ingest([ev("slack:C1:1.1", "message", actor="me", to_me=False, at="2026-10-21T13:50:00Z", thread="slack:C1")])
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["status"] == "done" and item["resolved"] == "closed"
    at(monkeypatch, "2026-10-21T10:30")
    feed.ingest([ev("slack:C1:1.2", "dm", at="2026-10-21T14:20:00Z", thread="slack:C1")])
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["status"] == "new" and len(fake.calls()) == 2


def test_web_api_lists_and_acts_with_the_same_actions(fake):
    fake.use(notify_min_priority="3")
    _agent_task()
    feed.ingest([ev("slack:C1:1.0", "dm")])
    notify.run(wait=True)
    v = server.notifications_view()
    by_kind = {r["kind"]: r for r in v["items"]}
    assert by_kind["suggestion"]["actions"] == ["accept", "snooze", "dismiss"] and by_kind["dm"]["actions"] == ["snooze", "dismiss"]
    assert by_kind["dm"]["link_label"] == "Open in Slack" and by_kind["suggestion"]["url"] == "#/today"
    assert v["status"]["enabled"] is True and v["status"]["unread"] == 2
    after = server.post(f"/api/notifications/{by_kind['dm']['id']}/snooze", {})
    assert next(r for r in after["items"] if r["kind"] == "dm")["status"] == "snoozed"
    after = server.post(f"/api/notifications/{by_kind['suggestion']['id']}/accept", {})
    assert next(r for r in after["items"] if r["kind"] == "suggestion")["status"] == "done"
    with pytest.raises(server.ApiError, match="already done"):
        server.post(f"/api/notifications/{by_kind['suggestion']['id']}/dismiss", {})
    with pytest.raises(server.ApiError, match="no notification"):
        server.post("/api/notifications/n-nope/read", {})


def test_marking_an_event_personal_removes_its_notification(fake):
    fake.use()
    feed.ingest([ev("slack:C1:1.0", "dm")])
    notify.run(wait=True)
    feed.mark("slack:C1:1.0", personal=True)
    assert notify.view()["items"] == []
    notify.run(wait=True)
    assert notify.load()["items"][0]["status"] == "done"


def test_cli_list_shows_titles_only_to_a_terminal(fake, capsys):
    fake.use()
    feed.ingest([ev("slack:C1:1.0", "dm", title="Secret title words")])
    assert main(["notify", "check", "--wait"]) == 0
    assert "1 new, 1 open; 1 pop-up (1 together)" in capsys.readouterr().out
    assert main(["notify", "list", "--show"]) == 0
    out = capsys.readouterr().out
    assert "Slack: a direct message asks you" in out and "next: reply in Slack" in out and "Secret title words" not in out
    assert main(["notify", "status"]) == 0
    assert "pop-ups: alerter: buttons Open, Accept, Dismiss, and Snooze" in capsys.readouterr().out


def test_events_are_content_free(fake, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    fake.use()
    feed.ingest([ev("slack:C1:1.0", "dm", title=f"{CANARIES[2]} title")])
    notify.run(wait=True)
    rows = [json.loads(x) for x in (home / "logs" / "events.jsonl").read_text().splitlines()]
    check = next(r for r in rows if r["event"] == "notify.check")
    assert check["status"] == "sent" and check["sent"] == 1 and check["new"] == 1 and check["source_kind"] == "alerter"
    assert CANARIES[2] not in (home / "logs" / "events.jsonl").read_text()


def test_the_watcher_cycle_checks_notifications(fake, monkeypatch):
    monkeypatch.setattr(watch, "notify", lambda text: None)  # the watcher's own count notifications (not this feature)
    called: list[bool] = []
    monkeypatch.setattr(notify, "run", lambda wait=False, now=None: called.append(True) or {})
    watch.run_cycle()
    assert called == [True]
    monkeypatch.setattr(notify, "run", lambda wait=False, now=None: 1 / 0)
    watch.run_cycle()  # a broken notifier never stops the cycle


def test_two_asks_on_one_pr_are_one_notification_and_your_review_closes_it(fake):
    """Asks with no chat thread share the PR's link key (`feed.thread_key`, issue #228), so they are one item."""
    fake.use(notify_min_priority="3")  # both asks pass the threshold
    pr = "https://github.example.com/acme/shop/pull/12"
    feed.ingest([ev("gh:rr-12", "review-request", source="github", thread=None, url=pr), ev("gh:c-12", "mention", source="github", thread=None, url=f"{pr}#issuecomment-3", at="2026-10-21T13:40:00Z")])
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["status"] == "new" and item["count"] == 2
    feed.ingest([ev("gh:rv-12", "review", source="github", actor="me", to_me=False, thread=None, url=f"{pr}/files#pullrequestreview-9", at="2026-10-21T13:50:00Z")])
    notify.run(wait=True)
    (item,) = notify.load()["items"]
    assert item["status"] == "done" and item["resolved"] == "closed"
