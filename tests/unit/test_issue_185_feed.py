"""Issue #185, part 5: "not an ask" and "done" marks close an ask, and repeated marks propose a feed rule."""

import json

import pytest

from careerbrew import signals, store
from careerbrew.cli import main
from careerbrew.day import inbox
from careerbrew.work import feed


@pytest.fixture
def asks(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    items = [{"id": f"slack:C1:{n}", "source": "slack", "kind": "mention", "at": f"2026-10-21T1{n}:00:00Z", "title": f"mention {n}", "actor": "other", "to_me": True, "channel": "release-bot", "thread": f"slack:C1:{n}"} for n in range(4)]
    items.append({"id": "slack:D9:1", "source": "slack", "kind": "dm", "at": "2026-10-21T10:00:00Z", "title": "DM from pat", "actor": "other", "to_me": True, "from": "pat", "thread": "slack:D9"})
    feed.add_lines("\n".join(json.dumps(i) for i in items))
    return home


def _open():
    return {r["id"] for r in feed.open_asks()}


def test_not_an_ask_and_done_close_an_ask_and_ask_opens_it_again(asks, capsys):
    assert "slack:C1:0" in _open()
    assert main(["work", "feed", "mark", "slack:C1:0", "not-ask"]) == 0
    assert "not an ask: it leaves your open asks (undo: careerbrew work feed mark slack:C1:0 ask)" in capsys.readouterr().out
    feed.mark_ask("slack:C1:1", "done")
    assert {"slack:C1:0", "slack:C1:1"}.isdisjoint(_open())
    feed.mark_ask("slack:C1:0", "ask")
    assert "slack:C1:0" in _open()
    with pytest.raises(feed.FeedError, match="an ask is marked not-ask, done, or ask"):
        feed.mark_ask("slack:C1:0", "maybe")


def test_a_mark_survives_the_next_pull_and_a_retriage(asks):
    feed.mark_ask("slack:C1:2", "not-ask")
    feed.add_lines(json.dumps({"id": "slack:C1:2", "source": "slack", "kind": "mention", "at": "2026-10-21T12:00:00Z", "title": "mention 2, edited", "actor": "other", "to_me": True, "channel": "release-bot"}))
    feed.retriage()
    assert "slack:C1:2" not in _open()


def test_three_not_an_ask_marks_in_one_channel_propose_an_fyi_rule(asks):
    for n in range(3):
        feed.mark_ask(f"slack:C1:{n}", "not-ask")
    (p,) = signals.feed_rule_proposals()
    assert p["rule"] == {"name": "not-ask-channel-release-bot", "when": {"source": "slack", "channel": "release-bot"}, "triage": "fyi"}
    inbox.regenerate()
    (item,) = [i for i in inbox.load() if i["kind"] == "feed-rule"]
    assert item["text"] == "Treat slack messages from #release-bot as fyi, not asks" and "you marked 3" in item["why"]
    inbox.accept(item["id"])
    assert any(r["name"] == "not-ask-channel-release-bot" for r in feed.rules() if not r["builtin"])
    assert "slack:C1:3" not in _open(), "the fourth mention is fyi now, by the new rule"
    assert signals.feed_rule_proposals() == [], "a rule the owner has is not proposed again"


def test_two_marks_are_not_a_rule(asks):
    feed.mark_ask("slack:C1:0", "not-ask")
    feed.mark_ask("slack:C1:1", "not-ask")
    assert signals.feed_rule_proposals() == []


def test_three_personal_marks_from_one_sender_propose_a_personal_rule(asks):
    more = [{"id": f"slack:D9:{n}", "source": "slack", "kind": "dm", "at": f"2026-10-21T1{n}:30:00Z", "title": "DM", "actor": "other", "to_me": True, "from": "pat"} for n in range(2, 4)]
    feed.add_lines("\n".join(json.dumps(i) for i in more))
    for eid in ("slack:D9:1", "slack:D9:2", "slack:D9:3"):
        feed.mark(eid, personal=True)
    (p,) = signals.feed_rule_proposals()
    assert p["kind"] == "personal" and p["rule"]["when"] == {"source": "slack", "from": "pat"} and p["rule"]["personal"] is True


def test_dismissed_ask_notifications_count_as_not_an_ask(asks):
    from careerbrew import notify

    items = [{"id": f"n-{n}", "key": f"feed:slack:C1:{n}", "refs": [f"slack:C1:{n}"], "kind": "mention", "sub": "mention", "status": "dismissed"} for n in range(3)]  # the shape notify._merge saves
    store.write_json(notify.path(), {"items": items, "popups": []})
    (p,) = signals.feed_rule_proposals()
    assert p["value"] == "release-bot" and p["count"] == 3


def test_a_broken_rules_file_never_breaks_the_inbox(asks):
    for n in range(3):
        feed.mark_ask(f"slack:C1:{n}", "not-ask")
    store.write_text(feed.rules_path(), "rules: [{when: {chanel: x}, triage: noise}]")
    assert signals.feed_rule_proposals() == []
    inbox.regenerate()  # does not raise


def test_an_accepted_rule_goes_first_and_keeps_the_owners_rules(asks):
    store.write_text(feed.rules_path(), "rules:\n  - {name: wide, when: {source: slack}, triage: noise}\n")
    for n in range(3):
        feed.mark_ask(f"slack:C1:{n}", "not-ask")
    (p,) = signals.feed_rule_proposals()
    inbox.regenerate()
    (item,) = [i for i in inbox.load() if i["kind"] == "feed-rule"]
    inbox.accept(item["id"])
    names = [r["name"] for r in feed.rules() if not r["builtin"]]
    assert names == ["not-ask-channel-release-bot", "wide"]


def test_a_sender_with_glob_characters_becomes_an_exact_rule(asks):
    assert signals._glob_escape("ops [bot]*") == "ops [[]bot[]][*]"


def test_accepting_a_rule_keeps_the_owners_comments(asks):
    store.write_text(feed.rules_path(), "# my rules: keep this note\nrules:\n  # pages are noise\n  - when: {source: pagerduty}\n    triage: noise\n")
    for n in range(3):
        feed.mark_ask(f"slack:C1:{n}", "not-ask")
    inbox.regenerate()
    (item,) = [i for i in inbox.load() if i["kind"] == "feed-rule"]
    inbox.accept(item["id"])
    text = store.read_text(feed.rules_path())
    assert "# my rules: keep this note" in text and "# pages are noise" in text and "rule-" not in text
    assert [r.get("name") for r in feed.rules() if not r["builtin"]][0] == "not-ask-channel-release-bot"


def test_a_rule_with_a_channel_list_covers_the_proposal_and_a_contrary_rule_does_not(asks):
    for n in range(3):
        feed.mark_ask(f"slack:C1:{n}", "not-ask")
    store.write_text(feed.rules_path(), "rules:\n  - {when: {source: slack, channel: [alerts, release-bot]}, triage: fyi}\n")
    assert signals.feed_rule_proposals() == []
    store.write_text(feed.rules_path(), "rules:\n  - {when: {source: slack, channel: release-bot}, triage: act}\n")
    assert len(signals.feed_rule_proposals()) == 1


def test_reopening_an_ask_drops_its_mark_time(asks):
    feed.mark_ask("slack:C1:0", "not-ask")
    feed.mark_ask("slack:C1:0", "ask")
    (row,) = [r for r in feed.load() if r["id"] == "slack:C1:0"]
    assert "ask_mark" not in row and "ask_marked_at" not in row
