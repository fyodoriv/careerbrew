"""Issue #147 (US-74): the suggestions inbox explains itself, groups alike rows, and hides asks done elsewhere.

Synthetic data only: ticket keys SYN-*, repo acme/shop, projects syn-*.
"""

from pathlib import Path

import pytest

from careerbrew import checklist, config, store
from careerbrew.cli import main
from careerbrew.day import done_elsewhere, inbox
from careerbrew.work import evidence, feed, priorities, projects, research

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"
LONG = "Rewrite the synthetic onboarding checklist so that every step names its owner, its due date, and the page it links to"


@pytest.fixture
def jira(home, monkeypatch):
    """A Jira connector on recorded answers: the search fixture lists three issues (PROJ-101..103)."""
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-07T09:00+00:00")
    (home / "work").mkdir(parents=True, exist_ok=True)
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "jira", "token": "env:CB_T", "base_url": "https://jira.example.com", "email": "me@example.com"}]})
    return home


def agent_task(title: str, why: str = "the synthetic sprint needs it", links: tuple[str, ...] = ()) -> dict:
    t = priorities.add(title, 3, 3, 1, [], source="agent", why=why, links=links)
    inbox.offer_task(t)
    return t


def proposals(kind: str = "") -> list[dict]:
    """Open proposals (of one kind). A cycle also proposes a promotion goal on a fresh data root: tests name the kind."""
    return [i for i in inbox.open_items() if i["type"] == "proposal" and (not kind or i["kind"] == kind)]


# ---------- each row explains itself ----------


def test_a_row_keeps_its_whole_title_and_says_where_it_came_from(home):
    t = agent_task(LONG, links=("ticket=https://tickets.example.com/browse/SYN-1",))
    (row,) = proposals()
    assert LONG in row["text"] and LONG in row["what"], "the whole title, never cut"
    assert "?" not in row["text"], "no cut marked with a question mark"
    assert row["added_by"] == "an agent"
    assert row["links"] == [{"label": "ticket", "url": "https://tickets.example.com/browse/SYN-1"}]
    assert row["lands"] == {"label": "Week", "url": "#/week"}, "every row links where Accept lands it"
    assert row["accept"].startswith("Moves the task from proposed to open") and "never planned" in row["dismiss"]
    assert row["data"]["task"] == t["id"]


def test_an_old_cut_title_is_replaced_by_the_whole_one_on_the_next_cycle(home):
    agent_task(LONG)
    items = inbox.load()
    items[0]["text"], items[0]["data"]["title"] = f"Plan t001 {LONG[:80]}? (added by agent: x)", LONG[:120]
    inbox._save(items)
    inbox.regenerate()
    (row,) = proposals("agent-task")
    assert LONG in row["text"] and row["data"]["title"] == LONG


def test_a_research_proposal_names_the_ticket_its_parent_and_links(jira):
    projects.add("syn-onboarding")
    projects.add_source("syn-onboarding", "tracker", "PROJ-7")
    research.refresh()
    inbox.regenerate()  # the watcher's next cycle
    rows = proposals("source")
    assert {r["data"]["ref"] for r in rows} == {"PROJ-101", "PROJ-102", "PROJ-103"}
    r = next(r for r in rows if r["data"]["ref"] == "PROJ-101")
    assert r["text"] == "Add tracker PROJ-101 to project syn-onboarding: Fix flaky payment retries", "the ticket's own title"
    assert r["what"] == r["text"], "the web app's What line shows the ticket's own title too, not only its key"
    (shown,) = [i for i in inbox.view()["inbox"] if i.get("data", {}).get("ref") == "PROJ-101"]
    assert shown["what"].endswith(": Fix flaky payment retries"), "Review each in the web app: each row's title"
    assert r["why"].startswith("PROJ-101 is a child of PROJ-7, which project syn-onboarding already follows"), "why it belongs, not the title again"
    assert r["added_by"] == "careerbrew: research on project syn-onboarding"
    assert r["links"][:2] == [{"label": "PROJ-101", "url": "https://jira.example.com/browse/PROJ-101"}, {"label": "PROJ-7 (parent)", "url": "https://jira.example.com/browse/PROJ-7"}]
    assert r["project"] == "syn-onboarding"


def test_a_ranked_project_gives_a_suggestion_its_project_and_priority(home):
    config.set_value("project_priority", "syn-platform=SYN-*, syn-other")
    agent_task("Close SYN-12 before the cutover")
    agent_task("Tidy the synthetic notes", why="another reason")
    rows = {r["data"]["title"]: r for r in proposals()}
    assert (rows["Close SYN-12 before the cutover"]["project"], rows["Close SYN-12 before the cutover"]["priority"]) == ("syn-platform", 1)
    assert (rows["Tidy the synthetic notes"]["project"], rows["Tidy the synthetic notes"]["priority"]) == ("", None)


# ---------- groups of alike suggestions ----------


def test_alike_research_proposals_form_one_group_and_accept_all_adds_each(jira):
    projects.add("syn-onboarding")
    projects.add_source("syn-onboarding", "tracker", "PROJ-7")
    research.refresh()
    inbox.regenerate()
    v = inbox.view()
    (g,) = [g for g in v["groups"] if g["kind"] == "source"]
    assert g["label"] == "Add 3 trackers to project syn-onboarding" and g["count"] == 3
    assert g["why"] == "each is a child of PROJ-7, which project syn-onboarding already follows"
    assert g["accept"].startswith("Adds all 3 to the sources of project syn-onboarding")
    assert g["links"] == [{"label": "PROJ-7 (parent)", "url": "https://jira.example.com/browse/PROJ-7"}], "the link every row shares"
    assert all(i["group"] == g["id"] for i in v["inbox"] if i["kind"] == "source")
    r = inbox.decide_group(g["id"], "accept", ids=g["ids"])
    assert r == {"group": g["id"], "action": "accept", "kind": "source", "decided": 3, "failed": [], "left": 0}
    refs = {s["ref"] for s in projects.show("syn-onboarding")["sources"]}
    assert {"PROJ-101", "PROJ-102", "PROJ-103"} <= refs
    assert not proposals("source"), "accepted: none left open"


def test_dismiss_all_dismisses_each_agent_task_and_a_smaller_set_stays_rows(home):
    for n in (1, 2, 3):
        agent_task(f"Synthetic chore {n}")
    agent_task("Synthetic chore with its own reason", why="a different reason")
    (g,) = inbox.view()["groups"]
    assert g["label"] == "Plan 3 tasks an agent added" and g["why"] == "an agent added them: the synthetic sprint needs it"
    assert "Sets all 3 tasks to dismissed" in g["dismiss"]
    inbox.decide_group(g["id"], "dismiss")
    assert sorted(t["status"] for t in priorities.load()) == ["dismissed", "dismissed", "dismissed", "proposed"]
    (left,) = proposals()
    assert "group" not in left, "one alike row is no group"


def test_accept_all_decides_only_the_rows_the_owner_confirmed(home):
    for n in (1, 2, 3):
        agent_task(f"Synthetic chore {n}")
    (g,) = inbox.view()["groups"]
    assert g["count"] == 3
    for n in (4, 5):  # the same reason: they join the same group id after the owner saw 3
        agent_task(f"Synthetic chore {n}")
    assert inbox.view()["groups"][0]["id"] == g["id"] and inbox.view()["groups"][0]["count"] == 5
    r = inbox.decide_group(g["id"], "accept", ids=g["ids"])
    assert (r["decided"], r["left"]) == (3, 2), "Accept all 3? accepts 3, not 5"
    assert sorted(t["title"] for t in priorities.load() if t["status"] == "proposed") == ["Synthetic chore 4", "Synthetic chore 5"]
    agent_task("Synthetic chore 6")  # 3 new alike rows: the same group id, none of them confirmed
    with pytest.raises(inbox.InboxError, match="none of the 3 suggestions you confirmed is still open"):
        inbox.decide_group(g["id"], "accept", ids=g["ids"])
    assert sum(1 for t in priorities.load() if t["status"] == "proposed") == 3, "nothing changed"


def test_the_group_size_is_a_setting(home):
    for n in (1, 2, 3):
        agent_task(f"Synthetic chore {n}")
    config.set_value("inbox_group_min", "4")
    assert inbox.view()["groups"] == []
    with pytest.raises(ValueError, match="from 2 to 100"):
        config.set_value("inbox_group_min", "1")


def test_accept_all_needs_yes_in_the_cli_and_an_agent_never_sees_titles(home, capsys):
    for n in (1, 2, 3):
        agent_task(f"Synthetic chore {n}")
    gid = inbox.view()["groups"][0]["id"]
    capsys.readouterr()
    assert main(["day", "accept-all", gid]) == 2
    out = capsys.readouterr().out
    assert "add --yes --count 3" in out and "Synthetic chore" not in out, "no title to an agent (stdout is not a terminal)"
    assert all(t["status"] == "proposed" for t in priorities.load()), "nothing changed without --yes"
    agent_task("Synthetic chore 4")  # joins the group after the owner saw 3
    capsys.readouterr()
    assert main(["day", "accept-all", gid, "--yes", "--count", "3"]) == 1
    assert "now holds 4 suggestions, not 3" in capsys.readouterr().out
    assert all(t["status"] == "proposed" for t in priorities.load()), "another count: nothing changed"
    assert main(["day", "accept-all", gid, "--yes", "--count", "4"]) == 0
    assert f"{gid}: 4 accepted" in capsys.readouterr().out
    assert all(t["status"] == "open" for t in priorities.load())
    assert main(["day", "accept-all", gid, "--yes"]) == 1, "a decided group is gone"


def test_the_web_app_needs_yes_for_a_group(home):
    from careerbrew import server

    for n in (1, 2, 3):
        agent_task(f"Synthetic chore {n}")
    (g,) = inbox.view()["groups"]
    gid = g["id"]
    with pytest.raises(server.ApiError, match="confirm it"):
        server.post(f"/api/inbox-group/{gid}/accept", {})
    with pytest.raises(server.ApiError, match="send their ids"):
        server.post(f"/api/inbox-group/{gid}/accept", {"yes": True})
    agent_task("Synthetic chore 4")  # joins the group while the page asks "Accept all 3?"
    out = server.post(f"/api/inbox-group/{gid}/accept", {"yes": True, "ids": g["ids"]})
    assert (out["result"]["decided"], out["result"]["left"]) == (3, 1) and out["groups"] == [] and out["inbox"] is not None
    assert [t["title"] for t in priorities.load() if t["status"] == "proposed"] == ["Synthetic chore 4"], "the row added since stays open"


# ---------- asks done elsewhere ----------


def feed_event(**kw) -> None:
    feed.ingest([{"source": "github", "title": "synthetic event", **kw}])


def test_a_review_you_gave_expires_the_review_ask_and_bring_back_keeps_it(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    agent_task("Review acme/shop#12 for the release", links=("https://github.com/acme/shop/pull/12",))
    feed_event(id="gh:review:1", at="2026-10-19T10:30", kind="review", actor="me", url="https://github.com/acme/shop/pull/12#pullrequestreview-1")
    inbox.regenerate()
    assert proposals("agent-task") == []
    (gone,) = [i for i in inbox.load() if i["kind"] == "agent-task"]
    assert gone["status"] == "expired" and gone["expired"]["what"] == "you gave a review (github)", "kept, never deleted"
    (shown,) = inbox.view()["expired"]
    assert shown["what"].startswith("Plan task") and shown["expired"]["url"].startswith("https://github.com/acme/shop/pull/12")
    inbox.reopen(gone["id"])
    inbox.regenerate()
    (back,) = proposals("agent-task")
    assert back["keep"] is True and "maybe_done" not in back, "the owner's choice wins: never hidden again"
    with pytest.raises(inbox.InboxError, match="only a suggestion careerbrew hid"):
        inbox.reopen(back["id"])


def test_an_unsure_match_only_says_maybe_done(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    agent_task("Write release notes for acme/shop#12")
    store.append_jsonl(evidence.evidence_path(), [{"id": "pr:acme/shop#12", "kind": "pr", "title": "Ship the cart", "url": "https://github.com/acme/shop/pull/12", "at": "2026-10-18T09:00:00Z", "state": "MERGED"}])
    inbox.regenerate()
    (row,) = proposals("agent-task")
    assert row["status"] == "open" and row["maybe_done"]["what"] == "PR acme/shop#12 merged", "notes are not a merge: unsure"


def test_ci_green_expires_a_ci_ask_only_while_it_is_the_latest_run(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    agent_task("Fix the failing build on acme/shop#12")
    feed_event(id="ci:1", at="2026-10-19T10:00", kind="ci", status="success", title="CI on acme/shop#12")
    feed_event(id="ci:2", at="2026-10-19T11:00", kind="ci", status="failure", title="CI on acme/shop#12")
    inbox.regenerate()
    assert [i["status"] for i in inbox.load() if i["kind"] == "agent-task"] == ["open"], "a later red run: still failing"
    feed_event(id="ci:3", at="2026-10-19T12:00", kind="ci", status="success", title="CI on acme/shop#12")
    inbox.regenerate()
    assert [i["status"] for i in inbox.load() if i["kind"] == "agent-task"] == ["expired"]


def test_a_review_before_the_ask_is_only_maybe_done(home, monkeypatch):
    feed_event(id="gh:review:0", at="2026-10-18T10:30", kind="review", actor="me", url="https://github.com/acme/shop/pull/12")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    agent_task("Review acme/shop#12 again")
    inbox.regenerate()
    (row,) = proposals("agent-task")
    assert row["maybe_done"]["what"] == "you gave a review (github)", "an older review may not answer a new ask"


def test_expiry_is_a_setting_and_only_uses_data_already_read(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    agent_task("Review acme/shop#12")
    feed_event(id="gh:review:1", at="2026-10-19T10:30", kind="review", actor="me", url="https://github.com/acme/shop/pull/12")
    config.set_value("inbox_expire_done", "false")
    inbox.regenerate()
    assert [i["status"] for i in inbox.load() if i["kind"] == "agent-task"] == ["open"]


def test_refs_read_links_pr_numbers_and_keys():
    assert done_elsewhere.refs_in("See https://github.com/Acme/Shop/pull/12/files and SYN-4, not UTF-8") == {
        "url:github.com/Acme/Shop/pull/12/files",
        "pr:acme/shop#12",
        "key:SYN-4",
    }
    assert done_elsewhere.refs_in("acme/shop#12 and https://jira.example.com/browse/SYN-9.") == {"pr:acme/shop#12", "url:jira.example.com/browse/SYN-9", "key:SYN-9"}


# ---------- getting started ----------


def test_the_checklist_marks_steps_from_data_and_hides_when_asked(home, capsys):
    c = checklist.build()
    assert (c["done"], c["total"], c["show"]) == (0, 6, True)
    priorities.add("Synthetic first task", 3, 3, 1, [])
    config.set_value("pm_slack", "#syn-updates")
    c = checklist.build()
    assert {s["id"] for s in c["steps"] if s["done"]} == {"task", "pm"}
    assert main(["checklist"]) == 0
    out = capsys.readouterr().out
    assert "getting started: 2 of 6 done" in out and "[x] Add your first task" in out and "[ ] Turn on the day watcher: careerbrew day watch on" in out
    config.set_value("first_run_checklist", "false")
    assert checklist.build()["show"] is False
