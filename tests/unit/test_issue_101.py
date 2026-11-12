"""Issue #101: a url source on a host careerbrew may not read (outside the plugin `allowed_hosts`) must not fail checkup forever."""

import pytest

from careerbrew import checkup, connectors
from careerbrew.cli import main
from careerbrew.work import projects, research

SSO_APP = "https://portal.sso.example.com/app/board/7"  # a company web app behind single sign-on (synthetic)
ALLOWED = "https://jira.example.com/wiki/spaces/BILL/overview"


def _plugin(tmp_path, agent_reads):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text(f"name: acme\nversion: 0.1.0\nagent_reads: [{agent_reads}]\npolicy:\n  allowed_hosts: [jira.example.com]\n")
    assert main(["plugin", "add", str(root)]) == 0


@pytest.fixture
def later(home, monkeypatch):
    """Sources are added on Oct 1; calling the fixture moves the clock past every stale limit. careerbrew never fetches here."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00+00:00")

    def no_fetch(url, headers=None):
        raise AssertionError(f"fetched {url}: careerbrew may not read this host")

    monkeypatch.setattr(connectors, "http_get", no_fetch)
    return lambda: monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00+00:00")


def test_a_url_outside_allowed_hosts_is_a_reference_you_read_not_a_checkup_failure(later, tmp_path):
    _plugin(tmp_path, "slack, gdocs")
    projects.add("billing")
    projects.add_source("billing", "url", SSO_APP)
    later()
    row = checkup.projects_step(refresh=False)
    assert row["ok"] and "(0 stale" in row["step"], row
    (src,) = projects.show("billing")["sources"]
    assert src["reference"] and not src["stale"] and "allowed_hosts" in src["reference_reason"]
    assert projects.reader("url", SSO_APP) == "you"
    assert projects.stale_sources() == []
    assert research.refresh() == {"refreshed": 0, "still_stale": 0, "skipped": 0, "proposed": []}


def test_a_url_on_an_allowed_host_is_still_tracked(later, tmp_path):
    _plugin(tmp_path, "slack, gdocs")
    projects.add("billing")
    projects.add_source("billing", "url", ALLOWED)
    later()
    assert [s["ref"] for s in projects.stale_sources()] == [ALLOWED]
    assert projects.reader("url", ALLOWED) == "careerbrew"


def test_agent_reads_url_hands_the_url_to_the_agent_and_checkup_passes(later, tmp_path):
    _plugin(tmp_path, "slack, gdocs, url")
    projects.add("billing")
    projects.add_source("billing", "url", SSO_APP)
    later()
    assert projects.reader("url", SSO_APP) == "agent"
    (stale,) = projects.stale_sources()
    assert stale["ref"] == SSO_APP and stale["reader"] == "agent", "the agent reads it, so it stays tracked"
    row = checkup.projects_step(refresh=False)
    assert row["ok"] and "1 more the agent reads itself" in row["step"], row
    assert research.refresh()["still_stale"] == 1, "careerbrew never fetches a url the agent reads"
    (src,) = projects.show("billing")["sources"]
    assert src["stale"] and not src["reference"] and "agent" in src["stale_reason"]


def test_the_users_choice_wins_over_the_host_default(later, tmp_path):
    _plugin(tmp_path, "slack, gdocs")
    projects.add("billing")
    projects.add_source("billing", "url", SSO_APP)
    later()
    projects.set_reference("billing", SSO_APP, on=False)
    assert [s["ref"] for s in projects.stale_sources()] == [SSO_APP], "tracked again: you read it, careerbrew reminds you"


def test_without_a_host_policy_any_url_stays_readable(later):
    projects.add("billing")
    projects.add_source("billing", "url", SSO_APP)
    later()
    assert projects.reader("url", SSO_APP) == "careerbrew"
    assert [s["ref"] for s in projects.stale_sources()] == [SSO_APP]


def test_cli_show_says_why_and_how_to_track_it(later, tmp_path, capsys):
    _plugin(tmp_path, "slack, gdocs")
    projects.add("billing")
    projects.add_source("billing", "url", SSO_APP)
    later()
    capsys.readouterr()
    assert main(["work", "project", "show", "billing"]) == 0
    out = capsys.readouterr().out
    assert f"ref    url      {SSO_APP}" in out and "read by you" in out and "allowed_hosts" in out and "--off" in out
    assert "STALE" not in out
