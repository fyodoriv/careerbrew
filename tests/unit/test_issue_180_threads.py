"""Issue #180, part 2: thread replies as their own events, teammates' own messages pulled, and teammate notes."""

import json

import pytest

from careerbrew import connectors
from careerbrew.cli import main
from careerbrew.work import feed, team

from .conftest import FIXTURES

TOK = "env:CB_TEST_TOKEN"


@pytest.fixture
def slack(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http" / "team-feed"))
    monkeypatch.setenv("CB_TEST_TOKEN", "test-token-not-real")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text(json.dumps({"connectors": [{"kind": "slack", "token": TOK}]}))
    team.add("Sam Lee", chat="U0SAM")
    return home


def _urls(monkeypatch):
    seen = []
    real = connectors.http_json
    monkeypatch.setattr(connectors, "http_json", lambda url, hdr=None, **kw: seen.append(url) or real(url, hdr, **kw))
    return seen


def test_each_reply_of_a_thread_is_its_own_event_under_the_parent(slack):
    items = {i["id"]: i for i in connectors.slack_feed({"kind": "slack", "token": TOK})}
    parent = "slack:C0ENG:1792591200.000100"
    replies = sorted(i for i, it in items.items() if it["thread"] == parent and i != parent)
    assert replies == ["slack:C0ENG:1792591260.000300", "slack:C0ENG:1792591320.000400"]
    assert items["slack:C0ENG:1792591320.000400"]["actor"] == "me" and items["slack:C0ENG:1792591260.000300"]["actor_id"] == "U0ANA"
    assert not any(it["thread"] == "slack:D0SAM" and i != "slack:D0SAM:1792591500.000200" for i, it in items.items()), "a DM is not read as a thread"


def test_a_teammates_messages_and_mentions_are_searched(slack, monkeypatch):
    seen = _urls(monkeypatch)
    connectors.slack_feed({"kind": "slack", "token": TOK})
    queries = [u for u in seen if "search.messages" in u]
    assert any("from%3A%3C%40U0SAM%3E" in u for u in queries) and any("query=%3C%40U0SAM%3E" in u for u in queries)
    team.remove("Sam Lee")
    seen.clear()
    connectors.slack_feed({"kind": "slack", "token": TOK})
    assert not any("U0SAM" in u for u in seen if "search.messages" in u)


def test_a_reply_that_mentions_a_teammate_is_kept_and_its_links_listed(slack):
    assert main(["work", "feed", "pull"]) == 0
    rows = {r["id"]: r for r in feed.load()}
    reply = rows["slack:C0ENG:1792591260.000300"]
    assert (reply["triage"], reply["team"]) == ("fyi", ["Sam Lee"])
    assert reply["links"] == ["https://tickets.example.com/PROJ-7"]


def test_without_the_history_scope_replies_are_skipped_and_the_pull_still_works(slack, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http" / "team-feed-noscope"))
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    items = connectors.slack_feed({"kind": "slack", "token": TOK})
    assert {i["id"] for i in items} == {"slack:C0ENG:1792591200.000100", "slack:D0SAM:1792591500.000200"}
    logged = [json.loads(line) for line in (slack / "logs" / "events.jsonl").read_text().splitlines() if '"work.feed.replies"' in line]
    assert logged[-1]["status"] == "error" and logged[-1]["error"] == "missing_scope"


def test_team_feed_groups_teammate_events_by_project_and_shows_research(slack, capsys, monkeypatch):
    from careerbrew.work import projects

    assert main(["work", "feed", "pull"]) == 0
    projects.add("export")
    projects.add_source("export", "chat", "#eng")
    feed.research("slack:C0ENG:1792591260.000300", [{"url": "https://tickets.example.com/PROJ-7", "summary": "the flag rollout ticket"}])
    rows = team.feed()
    assert {r["id"]: r["projects"] for r in rows}["slack:C0ENG:1792591260.000300"] == ["export"]
    assert {r["id"]: r["projects"] for r in rows}["slack:D0SAM:1792591500.000200"] == []
    capsys.readouterr()
    assert main(["work", "team", "feed"]) == 0
    out = capsys.readouterr().out
    assert "match a project" in out and "titles hidden" in out and "flag rollout" not in out
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)
    assert main(["work", "team", "feed"]) == 0
    out = capsys.readouterr().out
    assert "Sam Lee:" in out and "[export]" in out and "https://tickets.example.com/PROJ-7: the flag rollout ticket" in out


def test_a_teammates_note_line_names_them(slack):
    from careerbrew.work import memory_sync

    line = memory_sync.note_line({"id": "x", "source": "slack", "title": "ana in #eng", "at": "2026-10-21T14:01:00Z", "url": "https://example.slack.com/x", "team": ["Sam Lee"]})
    assert line.endswith("slack: Sam Lee: ana in #eng <https://example.slack.com/x>")
    assert memory_sync.SHAPES["work tools"].fullmatch(line), "the line keeps the shape careerbrew recognizes as its own"


def test_a_failed_replies_read_never_loses_the_pull(slack, monkeypatch):
    real = connectors.http_json

    def flaky(url, hdr=None, **kw):
        if "conversations.replies" in url:
            raise OSError("connection reset")
        return real(url, hdr, **kw)

    monkeypatch.setattr(connectors, "http_json", flaky)
    assert {i["id"] for i in connectors.slack_feed({"kind": "slack", "token": TOK})} == {"slack:C0ENG:1792591200.000100", "slack:D0SAM:1792591500.000200"}


def _stub(monkeypatch, fail):
    """http_json, with `fail(url)` returning a response (or raising) for the calls it picks, else the fixture."""
    real = connectors.http_json

    def call(url, hdr=None, **kw):
        got = fail(url)
        return real(url, hdr, **kw) if got is None else got

    monkeypatch.setattr(connectors, "http_json", call)


def test_a_rate_limited_team_search_keeps_your_own_asks(slack, monkeypatch):
    def fail(url):
        if "search.messages" in url and "U0SAM" in url:
            raise OSError("HTTP Error 429: Too Many Requests")

    _stub(monkeypatch, fail)
    ids = {i["id"] for i in connectors.slack_feed({"kind": "slack", "token": TOK})}
    assert "slack:D0SAM:1792591500.000200" in ids and "slack:C0ENG:1792591260.000300" in ids


def test_one_unreadable_thread_skips_only_that_thread(slack, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    roots = {
        "slack:C0PRIV:1792591100.000100": {"ts": "1792591100.000100", "channel": {"id": "C0PRIV", "name": "secret", "is_private": True}},
        "slack:C0ENG:1792591200.000100": {"ts": "1792591200.000100", "channel": {"id": "C0ENG", "name": "eng"}},
    }
    _stub(monkeypatch, lambda url: {"ok": False, "error": "missing_scope"} if "C0PRIV" in url else None)
    out = connectors._slack_replies({}, "U0ME", roots, "https://example.slack.com")
    assert {i["thread"] for i in out} == {"slack:C0ENG:1792591200.000100"}
    ok = [json.loads(line) for line in (slack / "logs" / "events.jsonl").read_text().splitlines() if '"work.feed.replies"' in line][-1]
    assert (ok["status"], ok["skipped"], ok["count"]) == ("ok", 1, 2)
    _stub(monkeypatch, lambda url: {"ok": False, "error": "ratelimited"})
    assert connectors._slack_replies({}, "U0ME", roots, "") == [], "a rate limit stops every thread"


def test_a_long_thread_follows_the_next_page(slack, monkeypatch):
    pages = iter(
        [
            {"ok": True, "has_more": True, "response_metadata": {"next_cursor": "c2"}, "messages": [{"ts": "1.000100", "user": "U0SAM"}, {"ts": "2.000100", "user": "U0SAM"}]},
            {"ok": True, "has_more": False, "messages": [{"ts": "3.000100", "user": "U0ANA"}]},
        ]
    )
    seen = []
    _stub(monkeypatch, lambda url: seen.append(url) or next(pages))
    out = connectors._slack_replies({}, "U0ME", {"slack:C0ENG:1.000100": {"ts": "1.000100", "channel": {"id": "C0ENG"}}}, "")
    assert [i["id"] for i in out] == ["slack:C0ENG:2.000100", "slack:C0ENG:3.000100"] and "cursor=c2" in seen[1]


def test_a_reply_gets_the_link_search_would_give_it(slack):
    items = {i["id"]: i for i in connectors.slack_feed({"kind": "slack", "token": TOK})}
    assert items["slack:C0ENG:1792591260.000300"]["url"] == "https://example.slack.com/archives/C0ENG/p1792591260000300?thread_ts=1792591200.000100&cid=C0ENG"


def test_a_reply_that_mentions_me_is_an_ask_until_i_reply_in_the_thread(slack, monkeypatch):
    replies = {"ok": True, "messages": [{"ts": "1792591200.000100", "user": "U0SAM"}, {"ts": "1792591260.000300", "user": "U0ANA", "text": "<@U0ME> can you check the flag?"}]}
    _stub(monkeypatch, lambda url: replies if "conversations.replies" in url else None)
    assert main(["work", "feed", "pull"]) == 0
    assert "slack:C0ENG:1792591260.000300" in {r["id"] for r in feed.open_asks()}
    replies["messages"].append({"ts": "1792591400.000500", "user": "U0ME", "text": "done"})
    assert main(["work", "feed", "pull"]) == 0
    assert "slack:C0ENG:1792591260.000300" not in {r["id"] for r in feed.open_asks()}


def test_team_searches_rotate_through_the_roster(slack, monkeypatch):
    for n in range(1, 12):
        team.add(f"Mate {n}", chat=f"U0M{n:02d}")
    seen = set()
    for minutes in range(0, 15 * 6, 15):
        monkeypatch.setenv("CAREERBREW_NOW", f"2026-10-21T{16 + minutes // 60:02d}:{minutes % 60:02d}")
        q = connectors._team_queries()
        assert len(q) <= 2 * connectors.TEAM_PER_PULL
        seen |= {x.split("@")[1].rstrip(">") for x in q}
    assert len(seen) == 12, "every teammate is searched within a few pulls"


def test_a_chat_id_must_look_like_one(slack):
    with pytest.raises(team.TeamError, match="Slack user id"):
        team.add("Ana", chat="U0ANA in:#secret")


@pytest.mark.parametrize("case", ["P01", "C01", "C02", "C03", "C04"])
def test_slack_duplicate_reply_binding(case, slack, monkeypatch, tmp_path):
    from urllib.parse import parse_qs, urlsplit

    from careerbrew import server

    channel = {"id": "C377", "name": "synthetic"}
    parent_ts = "1792591200.000100"
    reply_ts = "1792591260.000200"
    alternate_ts = "1792591140.000300"
    parent_id = f"slack:C377:{parent_ts}"
    reply_id = f"slack:C377:{reply_ts}"
    base = "https://example.slack.com"
    search_parent = {"C01": parent_ts, "C04": alternate_ts}.get(case)
    reply_url = f"{base}/archives/C377/p{reply_ts.replace('.', '')}"
    if search_parent:
        reply_url += f"?thread_ts={search_parent}&cid=C377"
    ask = {"ts": parent_ts, "channel": channel, "user": "U377ASK", "text": "<@U0ME> check the synthetic rollout"}
    reply = {
        "ts": reply_ts,
        "channel": channel,
        "user": "U0SAM" if case == "C03" else "U0ME",
        "text": "Synthetic response",
        "permalink": reply_url,
    }
    reply_search = "from:<@U0SAM>" if case == "C03" else "from:me"
    second = False
    calls = []

    def fixture(url, _headers=None, **_kwargs):
        parsed = urlsplit(url)
        assert parsed.netloc == "slack.com"
        query = parse_qs(parsed.query)
        if parsed.path == "/api/auth.test":
            return {"ok": True, "user_id": "U0ME", "url": base}
        if parsed.path == "/api/search.messages":
            search = query["query"][0].split(" after:", 1)[0]
            matches = [ask] if search == "to:me" else [reply] if second and search == reply_search else []
            return {"ok": True, "messages": {"matches": matches}}
        assert parsed.path == "/api/conversations.replies"
        assert query["channel"] == ["C377"]
        stamp = query["ts"][0]
        calls.append(stamp)
        assert stamp in (parent_ts, reply_ts)
        messages = [ask] if stamp == parent_ts else [reply]
        if second and stamp == parent_ts and case != "C02":
            messages.append({key: reply[key] for key in ("ts", "user", "text")})
        return {"ok": True, "messages": messages}

    monkeypatch.setattr(connectors, "http_json", fixture)
    first = feed.pull()
    assert (first["sources"], first["new"], first["errors"]) == (1, 1, 0)
    before = server.feed_asks()
    assert (before["total"], [row["id"] for row in before["asks"]]) == (1, [parent_id])
    second = True
    calls.clear()
    pulled = feed.pull()
    assert (pulled["sources"], pulled["new"], pulled["errors"]) == (1, 1, 0)
    assert parent_ts in calls
    stored = feed.load()
    assert {row["id"] for row in stored} == {parent_id, reply_id} and len(stored) == 2
    saved = next(row for row in stored if row["id"] == reply_id)
    after = server.feed_asks()
    visible = [row["id"] for row in after["asks"]]
    (tmp_path / "reply-binding.json").write_text(
        json.dumps({"case": case, "before_total": before["total"], "reply": saved, "after_total": after["total"], "visible_ids": visible, "reply_roots_requested": calls}, indent=2) + "\n",
        encoding="utf-8",
    )
    assert saved["url"] == reply_url and saved["source"] == "slack"
    assert saved["actor"] == ("other" if case == "C03" else "me")
    if case == "C04":
        assert saved["thread"] == f"slack:C377:{alternate_ts}"
    if case in {"P01", "C01"}:
        assert (after["total"], visible) == (0, []), f"stored reply thread: {saved['thread']}"
        assert saved["thread"] == parent_id
    else:
        assert (after["total"], visible) == (1, [parent_id])
