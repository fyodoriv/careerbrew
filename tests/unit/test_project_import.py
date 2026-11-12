"""Projects with a parent and links, and a bulk import from a sheet (issue #69, US-60)."""

from pathlib import Path

import pytest

from careerbrew import actions, plugins, server, vault
from careerbrew.cli import main
from careerbrew.work import project_import, projects

FIX = Path(__file__).resolve().parents[1] / "fixtures"
SHEET = FIX / "projects" / "team-goals.csv"


@pytest.fixture
def clock(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX / "http"))


def test_csv_import_sets_title_group_parent_links_and_trackers(clock):
    r = project_import.run(str(SHEET))
    assert r["rows"] == 6 and sorted(r["added"]) == ["billing", "checkout", "checkout-analytics", "invoices", "ledger", "payments"]
    assert r["sources"] == 5 and r["ignored"] == ["owner"], "BILL-1, BILL-2, BILL-7, PAY-1, CHK-3; the Owner column is named, not stored"
    b = projects.show("billing")
    assert (b["title"], b["group"], b["parent"], b["links"]) == ("Billing revamp", "q4-cutline", "payments", ["ledger", "invoices"])
    assert b["children"] == ["invoices"] and b["linked_from"] == ["ledger"]
    assert [s["ref"] for s in b["sources"] if s["kind"] == "tracker"] == ["BILL-1", "BILL-2"]
    assert projects.show("checkout-analytics")["parent"] == "checkout", "a sheet name like 'Checkout Analytics' becomes a project name"
    assert projects.show("payments")["children"] == ["billing", "ledger"]


def test_reimport_takes_non_empty_cells_and_keeps_sources_and_their_checks(clock, tmp_path):
    project_import.run(str(SHEET))
    projects.check("billing", "BILL-1", "epic 3 of 5")
    again = tmp_path / "again.csv"
    again.write_text("name,title,group,parent,links,trackers\nbilling,Billing revamp phase 2,,,,BILL-1 BILL-9\npayments,Payments platform,q4-cutline,,,PAY-1\n")
    r = project_import.run(str(again))
    assert (r["added"], r["updated"], r["unchanged"], r["sources"]) == ([], ["billing"], ["payments"], 1)
    b = projects.show("billing")
    assert b["title"] == "Billing revamp phase 2" and b["parent"] == "payments" and b["links"] == ["ledger", "invoices"], "empty cells keep what is there"
    one = next(s for s in b["sources"] if s["ref"] == "BILL-1")
    assert one["checked_at"] and one["status"] == "epic 3 of 5", "a re-import never resets a source's check"


def test_a_bad_sheet_writes_nothing_and_names_every_bad_row(clock, tmp_path):
    bad = tmp_path / "bad.csv"
    bad.write_text(
        "name,title,parent,links\n"
        "alpha,Alpha,beta,\n"  # row 2: alpha -> beta -> alpha
        "beta,Beta,alpha,\n"  # row 3
        "gamma,Gamma,nowhere,gamma\n"  # row 4: unknown parent, links to itself
        ",No name,,\n"  # row 5
        "alpha,Again,,\n"  # row 6: duplicate
        "delta,Delta,,ghost\n"  # row 7: unknown link
    )
    with pytest.raises(projects.ProjectError) as e:
        project_import.run(str(bad))
    msg = str(e.value)
    assert "nothing imported" in msg
    for part in ("parents loop", "row 4 (gamma): parent nowhere is not a project", "row 4 (gamma): a project cannot link to itself", "row 5: no name", "row 6: alpha is also row 2", "row 7 (delta): link ghost is not a project"):
        assert part in msg, part
    rows = [line.split(":")[0].split(" (")[0] for line in msg.splitlines()[1:]]
    assert rows == sorted(rows, key=lambda r: int(r.split()[1])), "problems are listed in sheet order"
    assert projects._load() == {}, "all or nothing"


def test_dry_run_checks_and_counts_but_writes_nothing(clock):
    r = project_import.run(str(SHEET), dry_run=True)
    assert r["dry_run"] and len(r["added"]) == 6
    assert projects._load() == {}


def test_json_rows_take_lists_or_split_text_and_link_to_projects_already_there(clock):
    project_import.run(str(SHEET))
    r = project_import.run(str(FIX / "projects" / "cutline.json"))
    assert sorted(r["added"]) == ["search", "search-ranking"] and r["unchanged"] == ["checkout"]
    s = projects.show("search-ranking")
    assert s["parent"] == "search" and [x["ref"] for x in s["sources"]] == ["SRCH-4", "SRCH-5"]
    assert projects.show("checkout")["linked_from"] == ["search"]


def test_a_google_sheets_link_reads_its_csv_export_without_signing_in(clock):
    project_import.run(str(SHEET))
    r = project_import.run("https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit#gid=0")
    assert r["added"] == ["reports"] and r["updated"] == ["billing"]
    assert projects.show("reports")["parent"] == "payments"
    assert project_import.sheet_csv_url("https://docs.google.com/spreadsheets/d/abc_1-x/edit?usp=sharing#gid=42") == "https://docs.google.com/spreadsheets/d/abc_1-x/export?format=csv&gid=42"
    assert project_import.sheet_csv_url("https://docs.google.com/spreadsheets/d/e/2PACX-pub/pubhtml") == "https://docs.google.com/spreadsheets/d/e/2PACX-pub/pub?output=csv"


def test_a_private_sheet_or_another_site_names_the_fix(clock, tmp_path):
    xlsx = tmp_path / "goals.xlsx"
    xlsx.write_bytes(b"PK\x03\x04\xff\xfe\x00binary")
    with pytest.raises(projects.ProjectError, match="goals.xlsx is not a text file.*CSV"):
        project_import.run(str(xlsx))
    with pytest.raises(projects.ProjectError, match="sheet is private.*File > Download > CSV"):
        project_import.run("https://docs.google.com/spreadsheets/d/sheet-private/edit")
    with pytest.raises(projects.ProjectError, match="Google Sheets link"):
        project_import.run("https://sheets.example.com/goals.csv")
    with pytest.raises(projects.ProjectError, match="no name column"):
        project_import.run(str(FIX / "http" / "docs.google.com" / "spreadsheets_d_sheet-private_export.html"))
    assert projects._load() == {}


def test_the_sheet_link_obeys_the_plugin_host_policy(clock, monkeypatch):
    monkeypatch.setattr(plugins, "effective_policy", lambda: {"allowed_hosts": ["example.com"]})
    with pytest.raises(RuntimeError, match="docs.google.com is not in the plugin policy allowed_hosts"):
        project_import.run("https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit")


def test_add_and_set_parent_and_links_refuse_unknown_names_and_loops(clock):
    projects.add("payments")
    projects.add("billing", parent="payments", links=["payments"], title="Billing revamp", group="q4")
    with pytest.raises(projects.ProjectError, match="parent nowhere is not a project"):
        projects.add("ledger", parent="nowhere")
    assert "ledger" not in projects._load()
    with pytest.raises(projects.ProjectError, match="parents loop"):
        projects.edit("payments", parent="billing")
    assert projects.show("payments")["parent"] == "", "a refused edit changes nothing"
    with pytest.raises(projects.ProjectError, match="link ghost is not a project"):
        projects.edit("billing", link=["ghost"])
    with pytest.raises(projects.ProjectError, match="does not link to ledger"):
        projects.edit("billing", unlink=["ledger"])
    projects.add("ledger")
    p = projects.edit("billing", parent="none", group="none", link=["ledger"], unlink=["payments"])
    assert "parent" not in p and "group" not in p and p["links"] == ["ledger"] and p["title"] == "Billing revamp"


def test_titles_are_encrypted_at_rest_and_kept_from_agents(clock, capsys, tmp_path):
    project_import.run(str(SHEET))
    canary = tmp_path / "canary.csv"
    canary.write_text("name,title,parent\nzebra,PROJECTZEBRA rollout,payments\n")
    assert main(["work", "project", "import", str(canary)]) == 0
    assert "PROJECTZEBRA" not in capsys.readouterr().out, "the import prints names and counts, never titles"
    raw = projects.path().read_bytes()
    assert raw.startswith(vault.MAGIC) and b"PROJECTZEBRA" not in raw and b"Billing revamp" not in raw
    assert main(["work", "project", "show", "zebra"]) == 0
    assert "PROJECTZEBRA" not in capsys.readouterr().out
    assert main(["work", "project", "show", "billing"]) == 0
    out = capsys.readouterr().out
    for part in ("group: q4-cutline", "parent: payments", "children: invoices", "links: ledger, invoices", "linked from: ledger"):
        assert part in out, part
    assert "Billing revamp" not in out, "a title is business text: an agent without a terminal does not see it"
    assert "title: Billing revamp" in actions.run("work.project.show", {"name": "billing"})["output"], "the owner's web app does"
    assert main(["work", "project", "list"]) == 0
    assert "parent payments" in capsys.readouterr().out


def test_import_from_the_web_app_asks_to_confirm(clock):
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("work.project.import", {"source": str(SHEET)})
    r = actions.run("work.project.import", {"source": str(SHEET), "dry_run": True}, confirm=True)
    assert r["exit"] == 0 and "6 row(s): 6 added" in r["output"] and "nothing written" in r["output"]


def test_work_view_lists_projects_as_a_tree(clock):
    project_import.run(str(SHEET))
    rows = server.work()["projects"]
    assert [(r["name"], r["depth"]) for r in rows] == [("payments", 0), ("billing", 1), ("invoices", 2), ("ledger", 1), ("checkout", 0), ("checkout-analytics", 1)], "by group, then parents before children"
    billing = next(r for r in rows if r["name"] == "billing")
    assert billing["title"] == "Billing revamp" and billing["links"] == ["ledger", "invoices"] and billing["children"] == 1 and billing["stale"] == 2
