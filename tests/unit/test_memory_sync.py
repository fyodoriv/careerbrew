"""Project memory sync: a memory file edit updates its project, and work-tool items that match a project
become dated notes in its memory file, once. Personal events never go there; careerbrew's own writes never
loop back."""

import json
import os
import stat
from datetime import date

import pytest

from careerbrew import config, plugins, store, vault
from careerbrew.cli import main
from careerbrew.day import watch
from careerbrew.work import evidence, feed, memory_sync, projects, research

NOW = "2026-10-21T16:00"
H2 = "##"  # a markdown section header, as careerbrew writes it into a memory file
EDIT = (
    "\nstatus: schema frozen, migration next week\n"
    "trackers: PROJ-7\n"
    "channels: #eng-billing\n"
    "links: search, no-such-project\n"
    "PROJ-44 is blocked on the UTF-8 fix (ISO-8601 dates)\n"
    "- design doc: https://docs.google.com/document/d/doc-billing-9/edit\n"
    "- repo: [billing](https://github.com/acme/billing).\n"
)


@pytest.fixture
def billing(home, tmp_path, monkeypatch):
    """An agent's notes file outside the data root, used as the billing project's memory."""
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    mem = tmp_path / "agent-memory" / "billing-research-log.md"
    mem.parent.mkdir(parents=True)
    mem.write_text("# billing research log\n\nNotes.\n")
    projects.add("search")
    projects.add_source("search", "tracker", "OPS-1")
    projects.add("billing", memory=str(mem))
    projects.add_source("billing", "tracker", "PROJ-1")
    return mem


def _sources(name):
    return {(s["kind"], s["ref"]) for s in projects._load()[name]["sources"]}


def _memory_source(name):
    return next(s for s in projects._load()[name]["sources"] if s["kind"] == "memory")


# ---------- in: a memory edit updates the project ----------


def test_a_memory_edit_updates_its_project(billing):
    billing.write_text(billing.read_text() + EDIT)
    r = memory_sync.sync_in()
    assert r["files"] == 1 and r["links"] == 1 and r["status"] == 1
    got = _sources("billing")
    assert {("tracker", "PROJ-7"), ("tracker", "PROJ-44"), ("chat", "#eng-billing"), ("doc", "doc-billing-9"), ("repo", "acme/billing")} <= got
    assert not {ref for _, ref in got} & {"UTF-8", "ISO-8601"}, "a ticket-shaped word whose prefix the project does not track is not a tracker"
    p = projects._load()["billing"]
    assert p["links"] == ["search"], "a link names an existing project; an unknown name is skipped"
    src = _memory_source("billing")
    assert src["status"] == "schema frozen, migration next week" and src["checked_at"]
    assert memory_sync.sync_in()["files"] == 0, "parsed once per change"


def test_a_change_is_found_by_content_hash_not_by_mtime(billing):
    memory_sync.sync_in()
    st = billing.stat()
    os.utime(billing, (st.st_atime, st.st_mtime + 3600))
    assert memory_sync.sync_in()["files"] == 0, "a touch with the same content is not a change"
    billing.write_text(billing.read_text() + "trackers: PROJ-8\n")
    os.utime(billing, (st.st_atime, st.st_mtime + 3600))  # same mtime as before the edit
    assert memory_sync.sync_in()["files"] == 1, "an edit that keeps the mtime is still a change"
    assert ("tracker", "PROJ-8") in _sources("billing")


def test_careerbrews_own_sections_are_never_parsed_back(billing):
    billing.write_text(
        billing.read_text()
        + f"\n{H2} 2026-10-20 work tools: 1 new item(s)\n\n- 2026-10-20 slack: Mention in #x <https://example.slack.com/archives/C0OTHER/p1>\n"
        + f"\n{H2} 2026-10-19 tracker PROJ-1: Billing\n\n- status: In Progress; updated 2026-10-19\n"
        + f"\n{H2} My notes\n\nstatus: green\n"
    )
    memory_sync.sync_in()
    assert ("chat", "C0OTHER") not in _sources("billing")
    assert _memory_source("billing")["status"] == "green", "the status comes from the owner's lines only"


def test_a_file_from_before_the_sync_reads_the_owners_bullets_under_careerbrews_old_sections(billing):
    billing.write_text(
        "# notes\n"
        f"\n{H2} 2026-10-01 doc doc-x: PRD\n\n- Decision: freeze\n- status: blocked on infra\n- repo: https://github.com/acme/ledger\n"
        f"\n{H2} 2026-10-02 tracker PROJ-1: Billing\n\n- status: In Progress; updated 2026-10-02\n- child PROJ-12: Done\n"
        f"\n{H2} 2026-10-03 chat #eng-billing: #eng-billing\n\n- 2 message(s) since the last check\n- https://example.slack.com/archives/C0BILL/p9\n"
        f"\n{H2} 2026-10-04 work tools: 1 new item(s)\n\n- 2026-10-04 slack: Mention in #x <https://example.slack.com/archives/C0OTHER/p1>\n- repo: https://github.com/acme/payments\n"
    )
    r = memory_sync.sync_in()
    got = _sources("billing")
    assert {("repo", "acme/ledger"), ("repo", "acme/payments")} <= got, "the owner's bullets right under careerbrew's old notes are read"
    assert _memory_source("billing")["status"] == "blocked on infra", "a tracker's status line careerbrew wrote is not the owner's status"
    assert not got & {("tracker", "PROJ-12"), ("chat", "C0BILL"), ("chat", "C0OTHER")}, "lines in the shape careerbrew writes are still its own"
    assert r["sources"] == 2 and r["status"] == 1


def test_a_careerbrew_write_made_while_the_sync_is_off_is_never_read_back(billing):
    memory_sync.sync_in()
    config.set_value("project_memory_sync", "false")
    billing.write_text(billing.read_text() + "trackers: PROJ-9\n")  # the owner's edit while the sync is off
    research.write_memory("billing", "2026-10-21 doc d1: PRD", ["see https://example.com/from-careerbrew"])
    config.set_value("project_memory_sync", "true")
    assert memory_sync.sync_in()["files"] == 1
    got = _sources("billing")
    assert ("url", "https://example.com/from-careerbrew") not in got, "careerbrew's write is still its own"
    assert ("tracker", "PROJ-9") in got, "the owner's edit from while the sync was off is read once it is on"


def test_a_project_with_memory_but_no_memory_source_gets_one(billing, tmp_path):
    other = tmp_path / "search-notes.md"
    other.write_text("status: indexing paused\n")
    projects.set_memory("search", str(other))  # set later: no memory source yet
    memory_sync.sync_in()
    assert ("memory", str(other)) in _sources("search")
    assert _memory_source("search")["status"] == "indexing paused"


def test_an_encrypted_memory_under_the_data_root_syncs_both_ways(billing, home):
    projects.add("ledger", memory="work/ledger-notes.md")
    store.write_text(home / "work" / "ledger-notes.md", "trackers: LED-4\n")
    memory_sync.sync_in()
    assert ("tracker", "LED-4") in _sources("ledger")
    feed.ingest([{"id": "u:1", "at": "2026-10-21T14:00:00Z", "title": "LED-4 cutover booked"}])
    assert memory_sync.sync_out(allowed=True)["notes"] == 1
    raw = (home / "work" / "ledger-notes.md").read_bytes()
    assert raw.startswith(vault.MAGIC) and b"cutover" not in raw, "still ciphertext on disk"
    assert "LED-4 cutover booked" in store.read_text(home / "work" / "ledger-notes.md")
    assert memory_sync.sync_in()["files"] == 0, "its own encrypted write is not a change"


# ---------- out: matching work-tool items become dated notes ----------


RULES = "rules:\n  - {name: family, when: {text: [dentist]}, personal: true}\n"


@pytest.fixture
def items(billing):
    projects.add_source("billing", "chat", "#eng-billing")
    projects.add_source("billing", "repo", "acme/billing")
    config.set_value("project_priority", "billing=*invoice*")
    store.write_text(feed.rules_path(), RULES)
    feed.ingest(
        [
            {
                "id": "slack:C0BILL:1792594800.000300",
                "source": "slack",
                "kind": "mention",
                "at": "2026-10-21T14:00:00Z",
                "title": "Mention from sam in #eng-billing",
                "channel": "eng-billing",
                "url": "https://example.slack.com/archives/C0BILL/p1",
                "to_me": True,
            },
            {"id": "pagerduty:Q9", "source": "pagerduty", "kind": "page", "at": "2026-10-21T15:00:00Z", "title": "Page: Invoice export failing", "url": "https://example.pagerduty.com/incidents/Q9", "to_me": True},
            {"id": "slack:C0RAND:2", "source": "slack", "kind": "message", "at": "2026-10-21T14:10:00Z", "title": "ana in #random", "channel": "random"},
            {"id": "slack:C0BILL:3", "source": "slack", "kind": "message", "at": "2026-10-21T14:20:00Z", "title": "monitor in #eng-billing", "channel": "eng-billing", "actor": "bot"},
            {"id": "slack:D0PAT:4", "source": "slack", "kind": "dm", "at": "2026-10-21T14:30:00Z", "title": "PROJ-1 retro moved", "text": "after the dentist"},
            {"id": "outlook:EVP", "source": "outlook", "kind": "meeting", "at": "2026-10-21T12:00:00Z", "title": "Organized: PROJ-1 sync", "actor": "me", "personal": True},
        ]
    )
    store.append_jsonl(
        evidence.evidence_path(),
        [
            {"id": "jira:PROJ-7", "kind": "jira", "title": "PROJ-7: Freeze the schema", "at": "2026-10-20T12:00:00.000+0000", "url": "https://jira.example.com/browse/PROJ-7"},
            {"id": "pr:acme/billing#41", "kind": "pr", "title": "feat: invoice export", "at": "2026-10-19T10:00:00Z", "url": "https://github.com/acme/billing/pull/41"},
            {"id": "jira:OPS-9", "kind": "jira", "title": "OPS-9: Reindex", "at": "2026-10-20T12:00:00Z", "url": "https://jira.example.com/browse/OPS-9"},
            {"id": "jira:PROJ-3", "kind": "jira", "title": "PROJ-3: Old work", "at": "2026-09-01T12:00:00Z", "url": "https://jira.example.com/browse/PROJ-3"},
            {"id": "slack:1792594800.000300", "kind": "slack", "title": "Recognition in #eng-billing", "at": "2026-10-21T14:00:00Z", "url": "https://example.slack.com/archives/C0BILL/p1"},
        ],
    )
    return billing


def _notes(path):
    return [ln for ln in path.read_text().splitlines() if ln.startswith("- 2026-")]


def test_matching_items_become_dated_notes_with_their_link_once(items):
    r = memory_sync.sync_out(allowed=True)
    assert r["notes"] == 4 and r["projects"] == 1
    assert _notes(items) == [
        "- 2026-10-19 pr: feat: invoice export <https://github.com/acme/billing/pull/41>",
        "- 2026-10-20 jira: PROJ-7: Freeze the schema <https://jira.example.com/browse/PROJ-7>",
        "- 2026-10-21 slack: Mention from sam in #eng-billing <https://example.slack.com/archives/C0BILL/p1>",
        "- 2026-10-21 pagerduty: Page: Invoice export failing <https://example.pagerduty.com/incidents/Q9>",
    ], "a repo, a tracker, a channel, and a priority pattern match; oldest first"
    text = items.read_text()
    assert f"{H2} 2026-10-21 work tools: 4 new item(s)" in text
    for absent in ("#random", "monitor", "OPS-9", "PROJ-3", "Recognition"):
        assert absent not in text, absent
    assert memory_sync.sync_out(allowed=True)["notes"] == 0, "re-collecting adds nothing"
    memory_sync.state_path().unlink()
    assert memory_sync.sync_out(allowed=True)["notes"] == 0, "a line already in the file is not added again"
    assert len(_notes(items)) == 4


def test_a_collect_with_nothing_new_rewrites_no_file(items):
    # The watcher runs this every cycle: a rewrite with the same content moved the web app's change version
    # (task watch-idle-no-change, US-64), so every live view refetched and every cached view rebuilt.
    from careerbrew import server

    memory_sync.sync_out(allowed=True)
    before = server.changes_view()["version"], server.files_version()
    assert memory_sync.sync_out(allowed=True)["notes"] == 0
    assert (server.changes_view()["version"], server.files_version()) == before


def test_personal_events_never_reach_memory(items):
    memory_sync.sync_out(allowed=True)
    text = items.read_text()
    assert "retro moved" not in text, "personal by a rule"
    assert "PROJ-1 sync" not in text, "personal by the source (a private entry)"


def test_an_event_marked_personal_later_leaves_memory(items):
    memory_sync.sync_out(allowed=True)
    row = feed.mark("slack:C0BILL:1792594800.000300", personal=True)
    assert row["memory_dropped"] == 1
    assert "Mention from sam" not in items.read_text()
    assert memory_sync.sync_out(allowed=True)["notes"] == 0, "it does not come back"
    store.write_text(feed.rules_path(), RULES + "  - {name: pages, when: {source: pagerduty}, personal: true}\n")
    feed.retriage()
    assert memory_sync.sync_out(allowed=True)["removed"] == 1, "a rule that makes it personal removes it at the next sync"
    assert "Invoice export failing" not in items.read_text()
    assert len(_notes(items)) == 2


MENTION = "slack:C0BILL:1792594800.000300"


def test_removing_a_note_keeps_the_files_mode(items):
    items.chmod(0o600)
    memory_sync.sync_out(allowed=True)
    assert feed.mark(MENTION, personal=True)["memory_dropped"] == 1
    assert stat.S_IMODE(items.stat().st_mode) == 0o600, "a private notes file stays private"


def test_an_agent_append_during_a_removal_is_kept(items, monkeypatch):
    memory_sync.sync_out(allowed=True)
    real = memory_sync._catch_up

    def agent_appends(key, text, st):  # an agent appends between careerbrew's read and its rewrite
        real(key, text, st)
        with items.open("a") as fh:
            fh.write("trackers: PROJ-9\n")

    monkeypatch.setattr(memory_sync, "_catch_up", agent_appends)
    assert feed.mark(MENTION, personal=True)["memory_dropped"] == 1
    monkeypatch.setattr(memory_sync, "_catch_up", real)
    text = items.read_text()
    assert "Mention from sam" not in text and text.endswith("trackers: PROJ-9\n")
    memory_sync.sync_in()
    assert ("tracker", "PROJ-9") in _sources("billing"), "the append is read at the next sync"


def test_a_file_rewritten_during_a_removal_is_left_alone_and_the_removal_retried(items, monkeypatch):
    memory_sync.sync_out(allowed=True)
    real = memory_sync._catch_up

    def owner_rewrites(key, text, st):
        real(key, text, st)
        items.write_text("# billing\n" + items.read_text())

    monkeypatch.setattr(memory_sync, "_catch_up", owner_rewrites)
    assert feed.mark(MENTION, personal=True)["memory_dropped"] == 0
    monkeypatch.setattr(memory_sync, "_catch_up", real)
    assert items.read_text().startswith("# billing\n# billing research log"), "the owner's rewrite is kept"
    assert memory_sync.sync_out(allowed=True)["removed"] == 1, "the next sync removes the note"
    assert "Mention from sam" not in items.read_text()


def test_business_reading_off_writes_no_notes(items):
    before = items.read_text()
    r = memory_sync.sync_out(allowed=False)
    assert r["notes"] == 0 and "business" in r["reason"]
    assert items.read_text() == before


def test_a_memory_index_never_gets_notes(items, tmp_path):
    index = tmp_path / "MEMORY.md"
    index.write_text("- [billing](billing.md)\n")
    every = projects._load()
    every["billing"]["memory"] = str(index)  # an old record from before the index was refused
    projects._save(every)
    assert memory_sync.sync_out(allowed=True)["notes"] == 0
    assert index.read_text() == "- [billing](billing.md)\n"


def test_the_setting_turns_both_directions_off(items):
    config.set_value("project_memory_sync", "false")
    items.write_text(items.read_text() + "trackers: PROJ-9\n")
    before = items.read_text()
    r = memory_sync.run(allowed=True)
    assert not r["enabled"] and r["in"]["files"] == 0 and r["out"]["notes"] == 0
    assert items.read_text() == before and ("tracker", "PROJ-9") not in _sources("billing")


# ---------- a child project with no memory file: its nearest parent's file ----------

LEDGER_MENTION = "slack:C0LED:1792594800.000400"


@pytest.fixture
def ledger(billing):
    """billing's child `ledger` has no notes path: its notes go into billing's file, as absorb's do."""
    projects.add("ledger", parent="billing")
    projects.add_source("ledger", "chat", "#eng-ledger")
    projects.add_source("ledger", "tracker", "LED-4")
    store.write_text(feed.rules_path(), RULES)
    feed.ingest(
        [
            {
                "id": LEDGER_MENTION,
                "source": "slack",
                "kind": "mention",
                "at": "2026-10-21T14:00:00Z",
                "title": "Mention from ana in #eng-ledger",
                "channel": "eng-ledger",
                "url": "https://example.slack.com/archives/C0LED/p1",
                "to_me": True,
            },
            {"id": "slack:D0PAT:5", "source": "slack", "kind": "dm", "at": "2026-10-21T14:30:00Z", "title": "LED-4 retro moved", "text": "after the dentist"},
        ]
    )
    store.append_jsonl(evidence.evidence_path(), [{"id": "jira:LED-4", "kind": "jira", "title": "LED-4: Ledger cutover", "at": "2026-10-20T12:00:00Z", "url": "https://jira.example.com/browse/LED-4"}])
    return billing


CHILD_NOTES = [
    "- 2026-10-20 jira: LED-4: Ledger cutover <https://jira.example.com/browse/LED-4>",
    "- 2026-10-21 slack: Mention from ana in #eng-ledger <https://example.slack.com/archives/C0LED/p1>",
]


def _tagged(path, name):
    """The note lines under careerbrew's `work tools` sections headed with this project's name."""
    out, inside = [], False
    for ln in path.read_text().splitlines():
        if ln.startswith("#"):
            inside = ln.startswith(f"{H2} ") and " work tools: " in ln and ln.endswith(f"(project {name})")
        elif inside and ln.startswith("- "):
            out.append(ln)
    return out


def test_a_child_without_notes_gets_its_notes_in_the_parents_file_headed_with_its_name(ledger, capsys, monkeypatch):
    from careerbrew import cli

    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert main(["work", "collect"]) == 0
    capsys.readouterr()  # a long run prints its page path and one summary line; `report latest` is the page as text
    assert main(["report", "latest"]) == 0
    assert "done     Updated project memory files: 2 note(s) written to 1 project(s)" in capsys.readouterr().out
    assert f"{H2} 2026-10-21 work tools: 2 new item(s) (project ledger)" in ledger.read_text()
    assert _tagged(ledger, "ledger") == CHILD_NOTES, "1 note per matched item, a Jira and a Slack one, oldest first"
    assert "retro moved" not in ledger.read_text(), "a personal event never reaches the parent's file"
    assert main(["work", "collect"]) == 0
    capsys.readouterr()
    assert main(["report", "latest"]) == 0
    assert "project memory" not in capsys.readouterr().out, "collecting again adds nothing"
    assert _notes(ledger) == CHILD_NOTES, "no note written twice"
    assert memory_sync.sync_in()["files"] == 0, "careerbrew's write into the parent's file is not read back"
    assert not _sources("billing") & {("chat", "C0LED"), ("tracker", "LED-4")}, "the child's notes never become the parent's sources"


def test_a_childs_note_leaves_the_parents_file_when_its_event_turns_personal(ledger):
    memory_sync.sync_out(allowed=True)
    assert feed.mark(LEDGER_MENTION, personal=True)["memory_dropped"] == 1
    assert _tagged(ledger, "ledger") == CHILD_NOTES[:1]
    assert memory_sync.sync_out(allowed=True)["notes"] == 0, "it does not come back"


def test_an_item_of_both_the_parent_and_the_child_is_the_parents_note(ledger):
    projects.add_source("billing", "chat", "#eng-ledger")  # the parent watches the same channel
    assert memory_sync.sync_out(allowed=True)["notes"] == 2
    text = ledger.read_text()
    assert f"{H2} 2026-10-21 work tools: 1 new item(s)\n\n{CHILD_NOTES[1]}\n" in text, "untagged: it is the parent's own item"
    assert _tagged(ledger, "ledger") == CHILD_NOTES[:1]


def test_an_item_of_two_children_names_both_once(ledger):
    projects.add("ledger-ui", parent="billing")
    projects.add_source("ledger-ui", "chat", "#eng-ledger")
    assert memory_sync.sync_out(allowed=True)["notes"] == 2
    assert f"{H2} 2026-10-21 work tools: 1 new item(s) (projects ledger, ledger-ui)\n\n{CHILD_NOTES[1]}\n" in ledger.read_text()
    assert _notes(ledger).count(CHILD_NOTES[1]) == 1


def test_a_childs_own_memory_file_wins(ledger, tmp_path):
    own = tmp_path / "ledger-notes.md"
    own.write_text("# ledger\n")
    projects.set_memory("ledger", str(own))
    assert memory_sync.sync_out(allowed=True)["notes"] == 2
    assert _notes(own) == CHILD_NOTES and "(project ledger)" not in own.read_text()
    assert _notes(ledger) == [], "nothing goes to the parent's file"


def test_a_child_whose_parent_uses_a_memory_index_gets_no_notes(ledger, tmp_path):
    index = tmp_path / "MEMORY.md"
    index.write_text("- [billing](billing.md)\n")
    every = projects._load()
    every["billing"]["memory"] = str(index)  # an old record from before the index was refused
    projects._save(every)
    assert memory_sync.sync_out(allowed=True)["notes"] == 0
    assert index.read_text() == "- [billing](billing.md)\n"


# ---------- no loop ----------


def test_the_watcher_ignores_its_own_writes(items):
    memory_sync.sync_in()
    assert memory_sync.sync_out(allowed=True)["notes"] == 4
    assert memory_sync.sync_in()["files"] == 0, "the hash careerbrew wrote is recorded"
    research.write_memory("billing", "2026-10-21 doc doc-billing-9: PRD", ["Decision: freeze on 2026-10-20 https://example.com/x"])
    assert memory_sync.sync_in()["files"] == 0, "absorb and checkup writes are recorded too"
    assert ("url", "https://example.com/x") not in _sources("billing")


def test_an_owner_line_appended_right_after_careerbrews_notes_is_read(items):
    memory_sync.sync_out(allowed=True)
    items.write_text(items.read_text() + "status: blocked on the infra review\n- repo: https://github.com/acme/ledger\n")
    memory_sync.sync_in()
    assert _memory_source("billing")["status"] == "blocked on the infra review", "the end of the file is not careerbrew's section"
    assert ("repo", "acme/ledger") in _sources("billing"), "a bullet right after careerbrew's bullets is the owner's"
    assert ("chat", "C0BILL") not in _sources("billing"), "careerbrew's own note links are still never read back"


def test_checkup_research_never_copies_a_memory_file_into_itself(items, monkeypatch):
    items.write_text(items.read_text() + "status: green\n")
    memory_sync.sync_in()
    memory_sync.sync_out(allowed=True)  # every note line holds a date, which research calls a key line
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T16:00")  # past the 7-day stale limit
    assert ("billing", "memory") in {(s["project"], s["kind"]) for s in projects.stale_sources()}
    before = items.read_text()
    research.refresh(read_allowed=True)
    assert items.read_text() == before, "no section copied from the memory file into itself"
    assert _memory_source("billing")["status"] == "green", "the owner's status stays"
    assert ("billing", "memory") not in {(s["project"], s["kind"]) for s in projects.stale_sources()}


def test_an_edit_made_just_before_a_careerbrew_write_is_not_lost(items):
    memory_sync.sync_in()
    items.write_text(items.read_text() + "trackers: PROJ-9\n")  # not read yet
    memory_sync.sync_out(allowed=True)
    assert ("tracker", "PROJ-9") in _sources("billing"), "the write read the edit first"


# ---------- the watcher, the CLI, and the event log ----------


def test_the_watcher_syncs_memory_each_cycle(items, monkeypatch):
    items.write_text(items.read_text() + "trackers: PROJ-9\n")
    assert watch.SOURCES["memory"](date(2026, 10, 21)) == []
    assert ("tracker", "PROJ-9") in _sources("billing")
    assert _notes(items) == [], "no plugin allows agents to read business data: no notes"
    monkeypatch.setattr(plugins, "effective_policy", lambda: {"agent_may_read_business": True})
    watch.SOURCES["memory"](date(2026, 10, 21))
    assert len(_notes(items)) == 4


def test_cli_sync_and_collect(items, capsys, monkeypatch):
    assert main(["work", "project", "sync"]) == 0
    out = capsys.readouterr().out
    assert "memory in: 1 changed file(s)" in out and "business reading is off" in out
    assert _notes(items) == []
    from careerbrew import cli

    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert main(["work", "collect"]) == 0
    capsys.readouterr()  # a long run prints its page path and one summary line; `report latest` is the page as text
    assert main(["report", "latest"]) == 0
    assert "done     Updated project memory files: 4 note(s) written to 1 project(s)" in capsys.readouterr().out
    assert main(["work", "project", "sync"]) == 0
    assert "memory in: 0 changed file(s)" in capsys.readouterr().out


def test_events_are_content_free(items, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    items.write_text(items.read_text() + EDIT)
    memory_sync.run(allowed=True)
    log = (home / "logs" / "events.jsonl").read_text()
    assert {"work.memory.in", "work.memory.out"} <= {json.loads(x)["event"] for x in log.splitlines()}
    for leak in ("eng-billing", "Invoice", "schema frozen", "PROJ-7", "acme"):
        assert leak not in log, leak
