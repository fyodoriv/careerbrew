"""Issue #268: promises the owner makes in sent messages become tracked projects until done."""

from __future__ import annotations

import json
from datetime import date

import pytest

from careerbrew import cli, server
from careerbrew.cli import main
from careerbrew.day import handoff
from careerbrew.work import feed, projects, promises

LINK = "https://slack.example.com/archives/D1/p2"
MONDAY = date(2026, 11, 2)


@pytest.fixture(autouse=True)
def clock(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00+00:00")


def _ask_and_reply(reply: str, kind: str = "dm", actor: str = "me", personal: bool = False) -> None:
    feed.ingest(
        [
            {"id": "slack:D1:1", "source": "slack", "kind": "dm", "at": "2026-11-02T08:00:00Z", "title": "DM from Sam", "actor": "other", "to_me": True, "from": "Sam", "thread": "slack:D1", "text": "the nightly job times out again"},
            {"id": "slack:D1:2", "source": "slack", "kind": kind, "at": "2026-11-02T08:05:00Z", "title": "You wrote in a DM", "actor": actor, "thread": "slack:D1", "url": LINK, "text": reply, "personal": personal},
        ]
    )


@pytest.mark.parametrize(
    ("text", "what", "due"),
    [
        ("Thanks! I'll look into the timeout.", "Look into the timeout", ""),
        ("I will send it tomorrow", "Send it tomorrow", "2026-11-03"),
        ("ok, I'm going to fix the flaky test by Friday!", "Fix the flaky test by Friday", "2026-11-06"),
        ("Let me check the logs today", "Check the logs today", "2026-11-02"),
        ("i'll write the doc next week", "Write the doc next week", "2026-11-09"),
        ("I can review it by 2026-11-04", "Review it by 2026-11-04", "2026-11-04"),
        ("Sure <@U1>, I'll ping <https://x.example.com/a|the owners> in 2 days", "Ping the owners in 2 days", "2026-11-04"),
    ],
)
def test_a_first_person_future_action_is_a_promise_with_its_due_day(text, what, due):
    assert promises.detect(text, MONDAY) == {"what": what, "due": due}


@pytest.mark.parametrize(
    "text",
    [
        "Let me know when it is done",  # asks the other person
        "I'll be out tomorrow",  # away, not an action
        "I will not do that",
        "I'll check.",  # names no action
        "I can review it",  # "I can" with no deadline offers; it does not promise
        "Can I'll do that?",
        "I can do that by Friday?",  # a question
        "The timeout fired again",
    ],
)
def test_other_text_is_not_a_promise(text):
    assert promises.detect(text, MONDAY) is None


def test_a_sent_message_i_ll_look_into_the_timeout_creates_a_tracked_promise_with_its_link():
    _ask_and_reply("Thanks! I'll look into the timeout.")
    rows = promises.open_list()
    assert len(rows) == 1
    r = rows[0]
    assert r["title"] == "Look into the timeout" and r["url"] == LINK and r["to"] == "Sam"
    assert r["event"] == "slack:D1:2" and r["next"] == "research" and r["closed"] == ""
    p = projects.records()[r["name"]]
    assert p["kind"] == "promise" and p["sources"] == [{"kind": "chat", "ref": LINK, "checked_at": None, "status": "", "reference": True}]
    assert projects.stale_sources() == []  # the link is a pointer: never stale, never read
    assert "look into" not in json.dumps(feed.load()).lower()  # the analysis still holds no message text


def test_closing_a_promise_removes_it_from_commitments_and_the_handoff(capsys, monkeypatch):
    _ask_and_reply("Thanks! I'll look into the timeout.")
    name = promises.open_list()[0]["name"]
    handoff.add("promises", "Look into the timeout", LINK)  # an agent also wrote it by hand: shown once
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert main(["day", "handoff", "show", "--today"]) == 0
    out = capsys.readouterr().out
    assert "Promises you made (1):" in out and f"[{name}] Look into the timeout (to Sam) {LINK}" in out
    assert [p["name"] for p in server.promises_view()["promises"]] == [name]
    assert main(["work", "promise", "done", name]) == 0
    assert promises.open_list() == [] and server.promises_view()["promises"] == []
    capsys.readouterr()
    assert main(["day", "handoff", "show", "--today"]) == 0
    assert "handoff: none" in capsys.readouterr().out
    assert promises.open_list(include_closed=True)[0]["closed"] == "done"


def test_a_promise_is_tracked_once_and_a_closed_one_never_comes_back():
    _ask_and_reply("I'll look into the timeout")
    name = promises.open_list()[0]["name"]
    promises.close(name, "dropped")
    _ask_and_reply("I'll look into the timeout")  # the next pull reads the same message again
    assert promises.scan() == {"events": 1, "new": 0}
    assert promises.open_list() == [] and len(promises.open_list(include_closed=True)) == 1


def test_only_my_own_non_personal_sent_messages_hold_promises():
    _ask_and_reply("I'll look into the timeout", actor="other")
    _ask_and_reply("I'll look into the timeout", personal=True)
    assert promises.open_list(include_closed=True) == []


def test_a_promise_on_a_known_project_is_a_task_under_it():
    projects.add("billing", title="Billing service")
    feed.ingest(
        [
            {
                "id": "slack:C1:9",
                "source": "slack",
                "kind": "message",
                "channel": "billing",
                "at": "2026-11-02T08:05:00Z",
                "title": "You wrote in #billing",
                "actor": "me",
                "url": LINK,
                "project": "Billing service",
                "text": "I'll send the rollout notes tomorrow",
            }
        ]
    )
    r = promises.open_list()[0]
    assert r["parent"] == "billing" and r["to"] == "#billing" and r["due"] == "2026-11-03"


def test_the_cycle_steps_research_draft_status_and_an_overdue_promise_comes_first(monkeypatch):
    _ask_and_reply("I'll look into the timeout")
    late = promises.add("Send the synthetic notes", to="Kim", due="2026-10-30")
    rows = promises.open_list()
    assert [r["name"] for r in rows][0] == late and rows[0]["overdue"] is True
    name = rows[1]["name"]
    promises.step(name, "research", "read the job log")
    assert promises.open_list()[1]["next"] == "draft"
    promises.step(name, "draft", "follow-up drafted for Sam")
    assert promises.open_list()[1]["next"] == "status"
    assert projects.records()[name]["updates"][-1]["text"] == "draft: follow-up drafted for Sam"
    with pytest.raises(projects.ProjectError, match="research, draft, status"):
        promises.step(name, "send", "x")


def test_an_agent_sees_names_and_dates_but_no_text(capsys, monkeypatch):
    _ask_and_reply("I'll look into the timeout")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert main(["work", "promise", "list", "--json"]) == 0
    row = json.loads(capsys.readouterr().out)["promises"][0]
    assert "title" not in row and "to" not in row and row["url"] == LINK and row["next"] == "research"
    assert main(["day", "handoff", "show", "--today"]) == 0
    out = capsys.readouterr().out
    assert "(text hidden)" in out and "timeout" not in out


def test_handoff_done_on_a_promise_closes_it(capsys):
    _ask_and_reply("I'll look into the timeout")
    name = promises.open_list()[0]["name"]
    assert main(["day", "handoff", "done", name]) == 0
    assert promises.open_list() == []


def test_bad_input_names_the_fix(capsys):
    assert main(["work", "promise", "done", "promise-00000000"]) == 2
    assert "no promise promise-00000000 (see: careerbrew work promise list)" in capsys.readouterr().out
    with pytest.raises(projects.ProjectError, match="looks like 2026-11-03"):
        promises.add("Send it", due="next friday")
    with pytest.raises(projects.ProjectError, match="http"):
        promises.add("Send it", link="javascript:alert(1)")


def test_a_broken_projects_file_never_loses_the_feed_events(monkeypatch):
    def boom(*_a, **_k):
        raise projects.ProjectError("broken")

    monkeypatch.setattr(projects, "add_promise", boom)
    _ask_and_reply("I'll look into the timeout")
    assert len(feed.load()) == 2


def test_one_failed_promise_never_drops_the_others_and_any_error_keeps_the_ingest(monkeypatch):
    real, calls = projects.add_promise, []

    def first_fails(name, **kw):
        calls.append(name)
        if len(calls) == 1:
            raise projects.ProjectError("broken")
        return real(name, **kw)

    monkeypatch.setattr(projects, "add_promise", first_fails)
    feed.ingest([{"id": f"slack:D1:{i}", "source": "slack", "kind": "dm", "at": f"2026-11-02T08:0{i}:00Z", "title": "You wrote in a DM", "actor": "me", "thread": "slack:D1", "text": f"I'll send note {i} today"} for i in (1, 2)])
    assert len(calls) == 2 and len(promises.open_list()) == 1
    monkeypatch.setattr(projects, "records", lambda: (_ for _ in ()).throw(RuntimeError("cannot decrypt")))
    feed.ingest([{"id": "slack:D1:3", "source": "slack", "kind": "dm", "at": "2026-11-02T08:03:00Z", "title": "x", "actor": "me", "text": "I'll send note 3 today"}])
    assert len(feed.load()) == 3


def test_a_promise_needs_no_notes_file_and_stays_out_of_the_work_project_lists():
    from careerbrew.day import inbox

    _ask_and_reply("I'll look into the timeout")
    name = promises.open_list()[0]["name"]
    assert not [q for q in inbox.candidates() if q.get("kind") == "memory"]
    assert name not in {p["name"] for p in projects.watch_list()} and name not in {p["name"] for p in projects.tree()}


def test_archiving_a_promise_as_a_project_closes_it_and_unarchive_opens_it_again():
    _ask_and_reply("I'll look into the timeout")
    name = promises.open_list()[0]["name"]
    projects.archive(name)
    assert promises.open_list() == [] and promises.open_list(include_closed=True)[0]["closed"] == "done"
    projects.archive(name, on=False)
    assert promises.open_list()[0]["closed"] == ""


def test_the_same_promise_added_twice_in_one_second_is_two_promises():
    a, b = promises.add("Send the notes"), promises.add("Send the notes")
    assert a != b and len(promises.open_list()) == 2
