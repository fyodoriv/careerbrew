"""Issue #72: one local event store for work tools. Originals encrypted, analysis apart, triage by rule,
a personal flag that never reaches the PM update, and Slack, PagerDuty, and Outlook connectors that write into it."""

import json
from datetime import date

import pytest

from careerbrew import calendar, config, connectors, plugins, store, vault
from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.day import results, watch
from careerbrew.work import evidence, feed

from .conftest import CANARIES, FIXTURES

DAY = date(2026, 10, 21)
TOK = "env:CB_TEST_TOKEN"
CONNECTORS = [{"kind": "slack", "token": TOK}, {"kind": "pagerduty", "token": TOK}, {"kind": "outlook", "token": TOK}]
RULES = """rules:
  - {name: family, when: {text: [dentist]}, personal: true}
  - {name: alerts, when: {source: slack, channel: 'alerts-*'}, triage: noise}
  - {name: helped, when: {source: slack, actor: me}, evidence: true}
"""


@pytest.fixture
def http(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http" / "feed"))
    monkeypatch.setenv("CB_TEST_TOKEN", "test-token-not-real")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")  # UTC-4: 20:00 UTC
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text(json.dumps({"connectors": CONNECTORS}))
    return home


def _rules(text=RULES):
    store.write_text(feed.rules_path(), text)


def _by_id():
    return {r["id"]: r for r in feed.load()}


def test_pull_keeps_originals_encrypted_and_the_analysis_apart(http):
    r = feed.pull()
    assert r == {"sources": 3, "new": 16, "updated": 0, "errors": 0, "reasons": []}
    raw = feed.originals_path().read_bytes()
    assert raw.startswith(vault.MAGIC) and CANARIES[2].encode() not in raw, "the original is ciphertext on disk"
    assert CANARIES[2] in store.read_text(feed.originals_path()), "the original text is kept for re-triage"
    analysis = store.read_text(feed.events_path())
    assert CANARIES[2] not in analysis and "rollout plan" not in analysis, "the analysis keeps no message text"
    assert all("text" not in row and "original" not in row for row in feed.load())
    assert feed.pull()["new"] == 0, "pulling again writes no duplicates"


def test_built_in_rules_triage_every_fixture_event(http):
    feed.pull()
    got = {i: r["triage"] for i, r in _by_id().items()}
    assert got == {
        "slack:D0ALEX:1792591200.000100": "act",  # a DM asks you
        "slack:D0ALEX:1792593000.000200": "mine",  # your reply
        "slack:C0BILL:1792594800.000300": "act",  # a mention asks you
        "slack:C0ALRT:1792595400.000400": "noise",  # a bot
        "slack:D0PAT:1792596000.000500": "mine",
        "pagerduty:Q1INC": "act",
        "pagerduty:Q2INC": "act",
        "pagerduty:R1ACK": "mine",
        "pagerduty:R2RES": "mine",
        "outlook:EVC": "fyi",  # an invite
        "outlook:EVA": "mine",  # you organized it
        "outlook:EVB": "mine",
        "outlook:mail:M4": "act",  # flagged
        "outlook:mail:M1": "act",  # important
        "outlook:mail:M3": "mine",  # you sent it
        "outlook:mail:M2": "noise",  # no-reply sender
    }
    rows = _by_id()
    assert rows["outlook:EVB"]["personal"] and rows["outlook:EVB"]["personal_by"] == "source", "Outlook marks it private"
    assert rows["pagerduty:R1ACK"]["evidence"] and rows["pagerduty:R1ACK"]["evidence_rule"] == "page-handled"
    assert rows["slack:C0ALRT:1792595400.000400"]["rule"] == "bot"


def test_an_ask_stays_open_until_you_reply_or_acknowledge(http):
    feed.pull()
    assert sorted(r["id"] for r in feed.open_asks()) == ["outlook:mail:M1", "pagerduty:Q2INC", "slack:C0BILL:1792594800.000300"]


def test_your_rules_run_first_and_retriage_uses_the_kept_originals(http):
    feed.pull()
    _rules()
    assert feed.retriage() == {"events": 16, "changed": 3}  # the dentist message, your reply, the alerts bot (now by your rule)
    rows = _by_id()
    assert rows["slack:D0PAT:1792596000.000500"]["personal"] and rows["slack:D0PAT:1792596000.000500"]["personal_by"] == "rule:family"
    assert rows["slack:D0ALEX:1792593000.000200"]["evidence"], "your rule makes your Slack replies evidence"
    assert not rows["slack:D0PAT:1792596000.000500"]["evidence"], "a personal event is never evidence, whatever the rules say"


def test_a_bad_rule_is_refused_with_the_fix(http):
    _rules("rules:\n  - {when: {chanel: x}, triage: noise}\n")
    with pytest.raises(feed.FeedError, match="unknown key chanel in when.*careerbrew data edit work/feed-rules.yaml"):
        feed.rules()
    _rules("rules:\n  - {when: {source: slack}, triage: urgent}\n")
    with pytest.raises(feed.FeedError, match="triage is one of act, fyi, noise, mine"):
        feed.rules()
    assert feed.pull()["errors"] == 3, "a broken rules file stops the pull and says so"


def test_personal_never_reaches_the_pm_update(http):
    _rules()
    watch.run_cycle()
    d = daylog.build(DAY)
    pm = "\n".join(daylog.pm_lines(d))
    log = daylog.render(d)
    assert "Dentist" not in log and "dentist" not in log, "the private meeting and the personal message stay out of the log"
    assert d["done"]["Meetings"] == ["Sprint planning", "Design review"]
    assert "led design review" in pm and "1 meeting," in pm and "dentist" not in pm.lower(), "issue #239: the meeting you organized is named, the other counted"
    assert "acknowledged page: checkout latency over slo" in pm.lower(), "your own page handling reaches the update as evidence"
    ids = {e["id"] for e in evidence.load()}
    assert "slack:D0PAT:1792596000.000500" not in ids and "outlook:EVB" not in ids
    personal_rows = [r for r in results.load(DAY) if r.get("personal")]
    assert [r["title"] for r in personal_rows] == ["Dentist"], "busy time for the planner, flagged personal"


def test_marking_an_event_personal_drops_its_evidence_and_its_pm_line(http):
    watch.run_cycle()
    assert "checkout latency" in "\n".join(daylog.pm_lines(daylog.build(DAY))).lower()
    assert main(["work", "feed", "mark", "pagerduty:R1ACK", "personal"]) == 0
    assert "pagerduty:R1ACK" not in {e["id"] for e in evidence.load()}
    assert "checkout latency" not in "\n".join(daylog.pm_lines(daylog.build(DAY))).lower(), "the row in today's results is skipped too"
    evidence.collect()
    assert "pagerduty:R1ACK" not in {e["id"] for e in evidence.load()}, "the owner's mark sticks on the next collect"
    feed.mark("pagerduty:R1ACK", personal=False)
    evidence.collect()
    assert "pagerduty:R1ACK" in {e["id"] for e in evidence.load()}


def test_your_mark_also_covers_the_same_event_as_slack_recognition_and_as_a_calendar_row(http):
    watch.run_cycle()
    pm = "\n".join(daylog.pm_lines(daylog.build(DAY)))
    assert "led design review" in pm and "1 meeting," in pm and "4 shout-outs in Slack" in pm
    feed.mark("slack:D0PAT:1792596000.000500", personal=True)  # the dentist message, also read as recognition
    feed.mark("outlook:EVC", personal=True)  # an invite, also a calendar row in today's results
    assert "slack:1792596000.000500" not in {e["id"] for e in evidence.load()}
    pm = "\n".join(daylog.pm_lines(daylog.build(DAY)))
    assert "led design review" in pm and "1 meeting," not in pm and "3 shout-outs in Slack" in pm, "the invite marked personal leaves; the led meeting stays"
    assert "Sprint planning" not in daylog.render(daylog.build(DAY))


def test_pagerduty_connector_turns_your_pages_into_ownership_evidence(http):
    items = connectors.run(CONNECTORS[1], "user")
    assert [i["title"] for i in items] == ["Acknowledged page: Checkout latency over SLO", "Resolved incident: Search index stale"]
    assert all(i["kind"] == "pagerduty" and "ownership" in i["tags"] for i in items)
    assert connectors.verify(CONNECTORS[1], "test-token-not-real") == {"ok": True, "message": "pagerduty: token works for Test User"}
    assert feed.load() == [], "the evidence path reads, it does not fill the store"


def test_outlook_evidence_skips_private_entries(http):
    assert [i["title"] for i in connectors.run(CONNECTORS[2], "user")] == ["Organized: Design review"]


def test_slack_recognition_skips_what_the_feed_keeps_personal(http):
    _rules()
    titles = [i["id"] for i in connectors.run(CONNECTORS[0], "user")]
    assert "slack:1792596000.000500" not in titles and len(titles) == 4


def test_policy_blocks_a_feed_connector(http, monkeypatch):
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {"allowed_connectors": ["slack"], "allowed_hosts": None})
    r = feed.pull()
    assert r["errors"] == 2 and all("not allowed by plugin policy" in x for x in r["reasons"])


def test_plugin_events_go_in_with_its_origin_and_leave_with_it(http, acme_plugin, tmp_path):
    good = tmp_path / "events.jsonl"
    good.write_text(
        json.dumps({"id": "doc-7", "at": "2026-10-21T15:00:00Z", "title": "Review asked on the roadmap doc", "source": "docs", "kind": "comment", "to_me": True, "text": f"{CANARIES[2]} roadmap"})
        + "\n"
        + json.dumps({"id": "cal-9", "at": "2026-10-21T16:00:00Z", "title": "School pickup", "source": "docs", "actor": "me", "personal": True})
        + "\n"
    )
    with pytest.raises(feed.FeedError, match="not installed"):
        feed.add_lines(good.read_text(), "acme")
    plugins.add(str(acme_plugin))
    bad = tmp_path / "bad.jsonl"
    bad.write_text(good.read_text() + '{"id": "x", "title": "no time"}\n')
    with pytest.raises(feed.FeedError, match="line 3: missing at"):
        feed.add_lines(bad.read_text(), "acme")
    assert feed.load() == [], "one bad line writes nothing"
    assert main(["work", "feed", "add", str(good), "--plugin", "acme"]) == 0
    rows = _by_id()
    assert set(rows) == {"acme:doc-7", "acme:cal-9"} and {r["origin"] for r in rows.values()} == {"plugin:acme"}
    assert rows["acme:doc-7"]["triage"] == "fyi" and rows["acme:cal-9"]["personal"]
    plugins.remove("acme", purge_data=True)
    assert feed.load() == [] and feed.originals() == {}, "plugin remove --purge-data deletes its events and originals"


def test_originals_are_dropped_after_feed_keep_days_and_the_analysis_stays(http):
    config.set_value("feed_keep_days", "1")
    feed.ingest([{"id": "old-1", "at": "2026-10-10T10:00:00Z", "title": "Old ask", "text": "old text"}, {"id": "new-1", "at": "2026-10-21T10:00:00Z", "title": "New ask", "text": "new text"}])
    assert set(feed.originals()) == {"new-1"} and {r["id"] for r in feed.load()} == {"old-1", "new-1"}


def test_cli_keeps_titles_and_canaries_off_a_piped_stdout(http, capsys, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    assert main(["work", "feed", "pull"]) == 0
    assert main(["work", "feed", "show", "--show"]) == 0
    assert main(["work", "feed", "open", "slack:C0BILL:1792594800.000300"]) == 0
    out = capsys.readouterr().out
    assert "pulled 3 source(s): 16 new, 0 updated, 0 error(s)" in out
    assert "act=6, fyi=1, noise=2, mine=6; open asks=3 (0 waiting over 8 work hours); evidence=2; personal=1" in out
    assert "titles hidden" in out and "hidden: stdout is not a terminal" in out
    assert CANARIES[2] not in out and "Checkout latency" not in out
    log = (http / "logs" / "events.jsonl").read_text()
    assert '"event": "work.feed.pull"' in log and CANARIES[2] not in log and "Checkout" not in log


def test_cli_lists_open_asks_to_the_owner(http, capsys, monkeypatch):
    feed.pull()
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)  # the owner's own terminal
    assert main(["work", "feed", "show", "--show", "--open"]) == 0
    out = capsys.readouterr().out
    assert "[act] pagerduty Page: Billing queue backlog" in out and "Mention from sam in #eng-billing" in out
    assert "rollout plan" not in out and "DM from alex" not in out, "an answered DM is not an open ask"


def test_private_ics_entries_are_personal():
    ics = "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:1\nDTSTART:20261021T140000Z\nDTEND:20261021T150000Z\nSUMMARY:Physio\nCLASS:PRIVATE\nEND:VEVENT\nEND:VCALENDAR\n"
    (ev,) = calendar.parse_ics(ics, DAY, calendar.local_tz())
    assert ev.personal and ev.busy
