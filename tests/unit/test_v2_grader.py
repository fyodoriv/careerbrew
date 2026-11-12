"""Regression tests for defects found by the v2 independent grader (2026-10-18)."""

import pytest

from careerbrew import calendar, connectors, machine, mail, plugins, prep, wizard
from careerbrew.cli import main
from careerbrew.day import watch
from careerbrew.work import priorities

from .conftest import CANARIES


def test_notification_redacts_terms_and_passes_text_as_argv(home, monkeypatch):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text(f"employer: {CANARIES[1]}\n")
    (home / "sensitive-terms.txt").write_text("PROJECTZEBRA\n")
    priorities.add(f'migrate PROJECTZEBRA for {CANARIES[1]} "quoted" \\', 3, 3, 0.25, [])
    sent = []
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setattr(watch.sys, "platform", "darwin")
    monkeypatch.setattr(watch.subprocess, "run", lambda cmd, **kw: sent.append(cmd))
    monkeypatch.setattr(watch.planner, "next_step", lambda: {"kind": "task", "task": "t001", "title": priorities.load()[0]["title"], "text": "Work on t001 now."})
    watch._maybe_suggest()
    (cmd,) = sent
    blob = " ".join(cmd)
    assert "PROJECTZEBRA" not in blob and CANARIES[1] not in blob
    assert cmd[-1].startswith("Next:"), "the message is an argv item, never spliced into AppleScript source"
    assert not any('display notification "' in part for part in cmd)


def test_switch_to_corporate_refuses_prep_and_linkedin_data_and_clears_job_outputs(home):
    main(["init"])
    main(["interview", "Stripe"])
    assert (home / "out" / "interview-stripe.md").exists()
    prep.add_story("s", ["leadership"])
    with pytest.raises(machine.RoleError, match="prep"):
        machine.set_role("corporate")
    (home / "prep" / "stories.yaml").unlink()
    machine.set_role("corporate")
    assert not (home / "out" / "interview-stripe.md").exists(), "derived job-hunt outputs are removed"
    assert not (home / "prep").exists()


def test_wizard_policy_keeps_outlook_and_calendar_hosts(home, tmp_path):
    wizard.new("AcmeCo", tmp_path / "p", ["jira"], {"jira_url": "https://jira.acme.example.com"}, prompt=False)
    hosts = plugins.effective_policy()["allowed_hosts"]
    for h in ("graph.microsoft.com", "api.fastmail.com", "calendar.google.com"):
        assert connectors.host_allowed(h), (h, hosts)


def test_ics_and_imap_obey_host_policy(home, acme_plugin, monkeypatch):
    m = acme_plugin / "careerbrew-plugin.yaml"
    m.write_text(m.read_text().replace("  allowed_routes: [local]", "  allowed_routes: [local]\n  allowed_hosts: [jira.example.com]"))
    plugins.add(str(acme_plugin))
    monkeypatch.setenv("CB_ICS", "https://calendar.evil.example.net/x.ics")
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text("calendars:\n  - {kind: ics, url: env:CB_ICS}\n")
    from datetime import date

    _evs, errors = calendar.events_for(date(2026, 10, 19))
    assert errors == ["ics: ConnectorError"]
    with pytest.raises(connectors.ConnectorError):
        mail.imap({"host": "imap.evil.example.net", "port": 993, "user": "env:X", "password": "env:X"}, 1)
    assert connectors.host_allowed("127.0.0.1"), "Proton Mail Bridge on localhost is always allowed"
