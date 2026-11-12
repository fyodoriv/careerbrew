"""Issue #70: a source careerbrew cannot read (a sign-in link) or one marked reference-only never goes stale."""

from datetime import date

import pytest

from careerbrew import checkup, connectors
from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.work import project_import, projects, research

SHEET = "https://docs.google.com/spreadsheets/d/sheet-goals-1/edit#gid=0"
OUTLOOK = "https://outlook.office.com/mail/inbox/id/AAQkSYNTH"
SLACK = "https://example.slack.com/archives/C0SYNTH/p1697000000000100"
SIGN_IN = (SHEET, OUTLOOK, SLACK)


@pytest.fixture
def later(home, monkeypatch):
    """Sources are added on Oct 1; calling the fixture moves the clock past every stale limit."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")

    def no_fetch(url, headers=None):
        raise AssertionError(f"fetched {url}: a sign-in link returns a sign-in page, not the content")

    monkeypatch.setattr(connectors, "http_get", no_fetch)
    return lambda: monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00+00:00")


def test_an_imported_goal_list_with_sign_in_links_leaves_only_readable_sources_stale(later, tmp_path):
    rows = [f"goal-{i},PROJ-{i} {SHEET}{i} {OUTLOOK}{i} {SLACK}{i}" for i in range(1, 13)]
    sheet = tmp_path / "goals.csv"
    sheet.write_text("name,trackers\n" + "\n".join(rows) + "\n")
    assert project_import.run(str(sheet))["sources"] == 48
    later()
    stale = projects.stale_sources()
    assert sorted(s["ref"] for s in stale) == sorted(f"PROJ-{i}" for i in range(1, 13)), "36 sign-in links are references"
    housekeeping = daylog.build(date(2026, 10, 20))["housekeeping"]
    assert housekeeping and not any(host in x for x in housekeeping for host in ("google.com", "office.com", "slack.com"))
    assert "(12 stale;" in checkup.projects_step(refresh=False)["step"]


def test_show_marks_sign_in_links_as_references_with_the_reason(later):
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    for ref in SIGN_IN:
        projects.add_source("billing", "url", ref)
    later()
    rows = {s["ref"]: s for s in projects.show("billing")["sources"]}
    assert rows["PROJ-1"]["stale"] and not rows["PROJ-1"]["reference"]
    for ref in SIGN_IN:
        assert rows[ref]["reference"] and not rows[ref]["stale"] and "sign-in" in rows[ref]["reference_reason"], ref
        assert projects.reader("url", ref) == "you"
    assert projects.reader("url", "https://example.com/post") == "careerbrew"
    assert projects.list_rows()[0]["stale"] == 1


def test_any_source_can_be_marked_reference_only_and_tracked_again(later):
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add_source("billing", "url", OUTLOOK)
    later()
    assert projects.set_reference("billing", "PROJ-1") == 1
    assert projects.stale_sources() == [], "a reference never goes stale"
    projects.set_reference("billing", OUTLOOK, on=False)
    assert [s["ref"] for s in projects.stale_sources()] == [OUTLOOK], "tracked again: you read it, careerbrew reminds you"
    projects.set_reference("billing", "PROJ-1", on=False)
    assert {s["ref"] for s in projects.stale_sources()} == {"PROJ-1", OUTLOOK}
    with pytest.raises(projects.ProjectError, match="has no source nope"):
        projects.set_reference("billing", "nope")


def test_absorb_keeps_a_sign_in_link_as_a_reference_and_refresh_never_fetches_it(later):
    projects.add("billing")
    r = research.absorb(OUTLOOK, "billing")
    assert not r["read"] and "reference" in r["reason"]
    (src,) = projects.show("billing")["sources"]
    assert src["reference"] and not src.get("stale_reason")
    later()
    assert research.refresh() == {"refreshed": 0, "still_stale": 0, "skipped": 0, "proposed": []}
    projects.set_reference("billing", OUTLOOK, on=False)
    assert research.refresh()["still_stale"] == 1, "tracked: refresh still never fetches a sign-in page"
    (src,) = projects.show("billing")["sources"]
    assert src["stale"] and "sign-in" in src["stale_reason"]


def test_cli_reference_action_and_show(later, capsys):
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add_source("billing", "url", SLACK)
    later()
    assert main(["work", "project", "reference", "billing", "PROJ-1"]) == 0
    assert "never goes stale" in capsys.readouterr().out
    out = (main(["work", "project", "show", "billing"]), capsys.readouterr().out)[1]
    assert "ref    tracker  PROJ-1" in out and f"ref    url      {SLACK}" in out and "STALE" not in out
    assert main(["work", "project", "reference", "billing", "PROJ-1", "--off"]) == 0
    assert "tracked again" in capsys.readouterr().out
