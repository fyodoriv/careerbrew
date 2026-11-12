"""Issue #228: an ask from a PR or a ticket has no chat thread. Its PR or ticket link is the thread key,
so a later action of yours on that PR or ticket closes the ask."""

import json

import pytest

from careerbrew.work import feed

PR = "https://github.example.com/acme/api/pull/12"
TICKET = "https://jira.example.com/browse/PROJ-7"


@pytest.fixture
def now(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    return home


def _add(*events):
    feed.add_lines("\n".join(json.dumps({"source": "github", "kind": "event", **e}) for e in events))


def _open():
    return sorted(r["id"] for r in feed.open_asks())


def test_your_review_on_the_pr_closes_its_review_request(now):
    _add({"id": "rr-12", "kind": "review-request", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "Review requested on PR #12", "url": PR})
    assert _open() == ["rr-12"], "a review request with no chat thread is an open ask"
    _add({"id": "rv-12", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You reviewed PR #12", "url": f"{PR}/files#pullrequestreview-9"})
    assert _open() == [], "your review on the same PR closes the ask"


def test_your_comment_on_the_ticket_closes_its_mention(now):
    _add({"id": "m-7", "source": "jira", "kind": "mention", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "You were mentioned on PROJ-7", "url": TICKET})
    _add({"id": "c-7", "source": "jira", "kind": "comment", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You commented", "url": f"{TICKET}?focusedCommentId=3"})
    assert _open() == []


def test_an_action_on_another_pr_or_before_the_ask_leaves_it_open(now):
    _add(
        {"id": "rv-old", "kind": "review", "actor": "me", "at": "2026-10-21T12:00:00Z", "title": "You reviewed PR #12", "url": PR},
        {"id": "rr-12", "kind": "review-request", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "Review requested on PR #12", "url": PR},
        {"id": "rv-120", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You reviewed PR #120", "url": f"{PR}0"},
        {"id": "rv-api2", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You reviewed api2 #12", "url": PR.replace("/api/", "/api2/")},
    )
    assert _open() == ["rr-12"]


def test_a_link_that_is_not_a_pr_or_ticket_is_no_thread_key(now):
    doc = "https://docs.example.com/roadmap"
    _add(
        {"id": "d-1", "source": "docs", "kind": "mention", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "Mentioned in the roadmap", "url": doc},
        {"id": "d-2", "source": "docs", "kind": "edit", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You edited the roadmap", "url": doc},
    )
    assert _open() == ["d-1"], "a doc edit is not an answer: only PR and ticket links are thread keys"


def test_a_chat_thread_still_wins_over_the_link(now):
    _add(
        {"id": "s-1", "source": "slack", "kind": "mention", "to_me": True, "at": "2026-10-21T13:00:00Z", "title": "Can you look at PR #12?", "url": PR, "thread": "slack:C1:1"},
        {"id": "rv-12", "kind": "review", "actor": "me", "at": "2026-10-21T14:00:00Z", "title": "You reviewed PR #12", "url": PR},
    )
    assert _open() == ["s-1"], "an ask in a chat thread closes by a reply in that thread"
