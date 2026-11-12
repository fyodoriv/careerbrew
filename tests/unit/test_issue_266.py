"""Issue #266: proactive listening. Every live cycle does the follow-up work for each new event, as drafts only:
a reply draft for each ask, research (or a reason) for each link, review notes for each review asked of you,
the event attached to its project, and one update line per goal for today. Nothing is ever sent."""

import http.client
import json
import os
from pathlib import Path

import pytest

from careerbrew import calendar, config, connectors, goals, store
from careerbrew.work import drafts, feed, listen, live, projects

FIX = Path(__file__).resolve().parents[1] / "fixtures"
LISTEN = FIX / "live" / "listen"


@pytest.fixture
def inbox(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX / "http"))
    d = tmp_path / "inbox"
    d.mkdir()
    live.start((LISTEN / "sources.yaml").read_text().replace("@DIR@", str(d)))
    _put(d, (LISTEN / "chat.json").read_text())
    return d


def _put(d, text):
    path = d / "chat.json"
    path.write_text(text)
    at = calendar.now_utc().timestamp()
    os.utime(path, (at, at))


def _goals() -> None:
    store.write_yaml(
        goals._path(),
        {
            "long_term": [{"id": "reviewer", "text": "Grow as a code reviewer", "by": None}],
            "queue": [{"id": "api-rollout", "kind": "custom", "title": "Ship the API rollout", "date": "2026-11-20", "topics": ["rollout"], "minutes_per_day": 30}],
            "history": [],
        },
    )


def _row(ts: str) -> dict:
    return next(r for r in feed.load() if r["id"] == f"chat:{ts}")


def _follow(rec: dict, ts: str) -> dict:
    return next(f for f in rec["followups"] if f["id"] == f"chat:{ts}")


ASK, REVIEW, NOTES, QUESTION, MINE, BOT = (
    "1792936800.000100",
    "1792936860.000200",
    "1792936920.000300",
    "1792936980.000400",
    "1792937040.000500",
    "1792937100.000600",
)


def test_a_cycle_with_a_direct_ask_stores_a_reply_draft(inbox):
    rec = live.cycle()
    f = _follow(rec, ASK)
    assert f["class"] == "needs you" and f["verdict"] == "answer"
    assert any(a.startswith("reply draft saved: chat:a1") for a in f["actions"]), f
    assert drafts.LocalBackend().read("chat:a1") == listen.HOLDING
    assert "never sent" in " ".join(rec["actions"])


def test_each_new_event_gets_a_class_and_the_action_taken(inbox):
    rec = live.cycle()
    classes = {f["id"].split(":")[1]: f["class"] for f in rec["followups"]}
    assert classes == {ASK: "needs you", REVIEW: "needs you", NOTES: "worth knowing", QUESTION: "worth knowing", MINE: "worth knowing", BOT: "noise"}
    assert all(f["actions"] for f in rec["followups"]), "every event names what was done, even 'nothing to do'"
    assert _follow(rec, BOT)["actions"] == ["nothing: noise"]


def _open_links(monkeypatch, addresses=("93.184.216.34",)) -> None:
    config.set_value("live_open_links", "true")
    monkeypatch.setattr(listen, "_addresses", lambda host: list(addresses))


def test_a_cycle_with_a_link_stores_research_or_a_reason(inbox, monkeypatch):
    _open_links(monkeypatch)
    live.cycle(read_allowed=True)
    budget = _row(ASK)["research"]
    assert budget == [{"url": "https://status.example.com/budget", "opened": True, "summary": "Error budget: Budget left: 40 percent; the freeze deadline is 2026-10-30", "by": "cycle"}]
    doc = _row(NOTES)["research"][0]
    assert doc["opened"] is False and "sign-in page" in doc["reason"] and doc["by"] == "cycle"
    pr = _row(REVIEW)["research"][0]
    assert pr["opened"] is False and "github connector" in pr["reason"]


def test_with_reading_off_a_link_gets_a_reason_and_stays_for_the_agent(inbox, monkeypatch):
    _open_links(monkeypatch)
    live.cycle()
    row = _row(ASK)
    assert row["research"][0]["opened"] is False and "reading business data is off" in row["research"][0]["reason"]
    assert feed.needs_research(row) == ["https://status.example.com/budget"], "a reason the cycle stored is a placeholder: the agent may still open it"
    feed.research(row["id"], [{"url": "https://status.example.com/budget", "summary": "budget is 40 percent"}])
    assert feed.needs_research(_row(ASK)) == []


def test_a_review_asked_of_you_gets_review_notes_drafted(inbox):
    rec = live.cycle(read_allowed=True)
    f = _follow(rec, REVIEW)
    target = listen.review_target(_row(REVIEW))
    assert f"review notes drafted: {target}" in f["actions"]
    notes = drafts.LocalBackend().read(target) or ""
    assert "https://github.com/example-shop/api/pull/9" in notes and "not posted" in notes and "Verdict:" in notes
    assert not any(a.startswith("review notes") for a in _follow(rec, ASK)["actions"]), "a plain ask is no review"


def test_an_event_is_attached_to_its_project_and_the_change_recorded(inbox):
    projects.add("shop-api")
    projects.add_source("shop-api", "repo", "example-shop/api")
    rec = live.cycle()
    assert _follow(rec, REVIEW)["project"] == "shop-api"
    assert "project shop-api updated" in _follow(rec, REVIEW)["actions"]
    ups = projects.records()["shop-api"].get("updates") or []
    assert len(ups) == 1 and ups[0]["kind"] == "project" and "review" in ups[0]["text"].lower()


def test_the_cycle_report_has_one_update_line_per_goal(inbox):
    _goals()
    rec = live.cycle()
    by = {g["id"]: g for g in rec["goals"]}
    assert set(by) == {"api-rollout", "reviewer"}, "one line per goal for today"
    g = by["api-rollout"]
    assert (g["moved"], g["waits_you"], g["waits_on"]) == (1, 1, ["Dee Moss"])
    assert listen.goal_line(g, show=True) == "goal api-rollout: 1 moved (your actions today), 2 other update(s); waits: 1 on you, 1 on Dee Moss"
    assert listen.goal_line(g, show=False) == "goal api-rollout: 1 moved (your actions today), 2 other update(s); waits: 1 on you, 1 on others"
    assert listen.goal_line(by["reviewer"], show=True) == "goal reviewer: nothing moved today; nothing waits"


def test_a_past_goal_gets_no_line_and_no_goals_means_no_lines(inbox):
    store.write_yaml(goals._path(), {"long_term": [{"id": "old", "text": "Old goal", "by": "2026-01-01"}], "queue": [], "history": []})
    assert live.cycle()["goals"] == []


def test_a_new_event_is_followed_up_once_and_an_existing_draft_is_kept(inbox):
    drafts.replace("chat:a1", "my own words, written earlier")
    projects.add("shop-api")
    projects.add_source("shop-api", "repo", "example-shop/api")
    rec = live.cycle()
    assert "draft kept: chat:a1 already has one" in _follow(rec, ASK)["actions"]
    assert drafts.LocalBackend().read("chat:a1") == "my own words, written earlier"
    _put(inbox, (LISTEN / "chat.json").read_text() + " ")
    again = live.cycle()
    assert again["followups"] == [], "the same events again are no new events"
    assert len(projects.records()["shop-api"]["updates"]) == 1


def test_a_small_ack_gets_an_emoji_suggestion_not_a_draft(inbox):
    _put(inbox, json.dumps({"messages": [{"ts": "1792937200.000700", "text": "thanks!", "actor": "other", "kind": "dm", "to_me": True, "thread": "chat:k1", "from": "Ana Ruiz"}]}))
    rec = live.cycle()
    assert _follow(rec, "1792937200.000700")["actions"] == ["react :pray: (no text draft)"]
    assert drafts.LocalBackend().read("chat:k1") is None


def test_the_cycle_never_sends_and_keeps_text_out_of_the_record_and_stdout(inbox, capsys):
    from careerbrew import cli

    _goals()
    assert cli.main(["work", "live", "cycle"]) == 0
    out = capsys.readouterr().out
    assert "needs you" in out and "reply draft saved: chat:a1" in out
    assert "goal api-rollout: 1 moved" in out and "1 on others" in out
    for secret in ("LISTENZEBRA", "Dee Moss", "error budget", "Ana Ruiz"):
        assert secret not in out, secret
    record = json.dumps(store.read_jsonl(live.cycles_path())[-1])
    assert "LISTENZEBRA" not in record and "error budget" not in record, "the cycle record holds ids, counts, and names, never message text"
    assert "LISTENZEBRA" not in (store.read_text(drafts.LocalBackend().path("chat:a1")) or "")


def test_draft_show_prints_the_text_only_where_business_data_may_show(inbox, capsys, monkeypatch):
    from careerbrew import cli

    live.cycle()
    assert cli.main(["draft", "show", "chat:a1"]) == 0
    out = capsys.readouterr().out
    assert listen.HOLDING not in out and f"{len(listen.HOLDING)} characters" in out
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["draft", "show", "chat:a1"]) == 0
    assert listen.HOLDING in capsys.readouterr().out
    assert cli.main(["draft", "show", "chat:none"]) == 1


def _messages(d, msgs) -> None:
    _put(d, json.dumps({"messages": msgs}))


def _msg(ts: str, text: str, **kw) -> dict:
    return {"ts": ts, "text": text, "actor": "other", "kind": "dm", "to_me": True, "thread": f"chat:{ts[-3:]}", "from": "Zed", **kw}


def test_a_poison_event_between_two_good_ones_fails_alone_and_a_repull_adds_nothing(inbox):
    projects.add("shop-api")
    projects.add_source("shop-api", "repo", "example-shop/api")
    good = "please review https://github.com/example-shop/api/pull/"
    _messages(inbox, [_msg("1792937300.000901", good + "1"), _msg("1792937301.000902", "see https://[draft"), _msg("1792937302.000903", good + "2")])
    rec = live.cycle()
    assert rec["status"] == "ok" and len(rec["followups"]) == 3, rec["followups"]
    assert all(_follow(rec, ts)["actions"][0].startswith("reply draft saved") for ts in ("1792937300.000901", "1792937302.000903"))
    assert "[draft" not in json.dumps(rec), "an error names its class, never the text"
    ups = len(projects.records()["shop-api"]["updates"])
    _put(inbox, (inbox / "chat.json").read_text() + " ")
    assert live.cycle()["followups"] == [] and len(projects.records()["shop-api"]["updates"]) == ups


def test_an_error_in_follow_up_work_never_stops_the_cycle_mark(inbox, monkeypatch):
    def boom(*_a, **_k):
        raise http.client.IncompleteRead(b"")

    monkeypatch.setattr(listen, "goal_updates", boom)
    monkeypatch.setattr(listen, "draft_reply", boom)
    rec = live.cycle()
    assert rec["status"] == "ok" and rec["n"] == 1
    assert _follow(rec, ASK)["actions"][-1] == "failed: IncompleteRead"
    assert live.cycle()["followups"] == [], "the failed events are followed, so a re-pull does not retry them forever"


def test_an_http_protocol_error_while_reading_a_link_is_a_reason(inbox, monkeypatch):
    _open_links(monkeypatch)

    def bad(*_a, **_k):
        raise http.client.BadStatusLine("x")

    monkeypatch.setattr(connectors, "http_get", bad)
    live.cycle(read_allowed=True)
    e = _row(ASK)["research"][0]
    assert e["opened"] is False and "BadStatusLine" in e["reason"]


def test_with_live_open_links_off_nothing_is_fetched(inbox, monkeypatch):
    def never(*_a, **_k):
        raise AssertionError("fetched a link with live_open_links off")

    monkeypatch.setattr(connectors, "http_get", never)
    monkeypatch.setattr(connectors, "http_json", never)
    assert config.get("live_open_links") is False
    live.cycle(read_allowed=True)
    for ts in (ASK, REVIEW):
        e = _row(ts)["research"][0]
        assert e["reason"] == listen.LINKS_OFF and e["by"] == "cycle"
    assert feed.needs_research(_row(ASK)) == ["https://status.example.com/budget"]


@pytest.mark.parametrize("url,addr", [("http://127.0.0.1:8080/x", "127.0.0.1"), ("http://10.1.2.3/y", "10.1.2.3"), ("http://localhost/z", "127.0.0.1"), ("http://intranet.example.com/a", "192.168.1.5")])
def test_with_live_open_links_on_a_private_or_loopback_link_is_skipped(inbox, monkeypatch, url, addr):
    _open_links(monkeypatch, (addr,))
    monkeypatch.setattr(connectors, "http_get", lambda *_a, **_k: (_ for _ in ()).throw(AssertionError("fetched a private address")))
    _messages(inbox, [_msg("1792937300.000901", f"look {url}")])
    live.cycle(read_allowed=True)
    e = _row("1792937300.000901")["research"][0]
    assert e["opened"] is False and "private or local address" in e["reason"]


def test_a_large_page_is_cut_at_the_size_cap(inbox, monkeypatch):
    _open_links(monkeypatch)
    monkeypatch.setattr(connectors, "http_get", lambda *_a, **_k: b"<title>Big</title>" + b"x" * (listen.MAX_PAGE_BYTES * 2))
    seen = []
    real = listen._summary_of

    def spy(html: str, ref: str):
        seen.append(len(html))
        return real(html, ref)

    monkeypatch.setattr(listen, "_summary_of", spy)
    live.cycle(read_allowed=True)
    assert seen and max(seen) <= listen.MAX_PAGE_BYTES


def test_a_link_marked_as_a_project_reference_is_not_opened(inbox, monkeypatch):
    _open_links(monkeypatch)
    projects.add("ops")
    projects.add_source("ops", "url", "https://status.example.com/budget")
    projects.set_reference("ops", "https://status.example.com/budget")
    monkeypatch.setattr(connectors, "http_get", lambda *_a, **_k: (_ for _ in ()).throw(AssertionError("opened a reference")))
    live.cycle(read_allowed=True)
    assert "reference" in _row(ASK)["research"][0]["reason"]


def test_one_project_update_per_project_per_cycle(inbox):
    projects.add("shop-api")
    projects.add_source("shop-api", "repo", "example-shop/api")
    link = "https://github.com/example-shop/api/pull/"
    _messages(inbox, [_msg(f"179293730{n}.00090{n}", f"see {link}{n}", kind="message", to_me=False) for n in range(3)])
    live.cycle()
    ups = projects.records()["shop-api"]["updates"]
    assert len(ups) == 1 and "3 event(s)" in ups[0]["text"]


def test_an_answered_ask_gets_no_review_notes(inbox):
    _messages(
        inbox,
        [
            _msg("1792937300.000901", "please review https://github.com/example-shop/api/pull/9", thread="chat:q1"),
            {"ts": "1792937360.000902", "text": "on it", "actor": "me", "kind": "dm", "thread": "chat:q1", "from": "owner"},
        ],
    )
    rec = live.cycle()
    f = _follow(rec, "1792937300.000901")
    assert f["verdict"] == "done" and not any(a.startswith("review notes") for a in f["actions"])


def test_replace_if_empty_keeps_the_owners_draft_under_the_lock():
    drafts.replace("chat:x1", "mine")
    assert drafts.replace("chat:x1", "the cycle's", if_empty=True)["kept"] is True
    assert drafts.LocalBackend().read("chat:x1") == "mine"
    assert drafts.replace("chat:x2", "the cycle's", if_empty=True)["kept"] is False
