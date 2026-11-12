import re

import yaml

from careerbrew import machine
from careerbrew.cli import main
from careerbrew.day import results, watch
from careerbrew.jobs import sources

from .conftest import CANARIES, FIXTURES


def _mail(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_G", "x")
    monkeypatch.setenv("CB_FM", "x")
    home.mkdir(parents=True, exist_ok=True)
    (home / "mail.yaml").write_text(yaml.safe_dump({"accounts": [{"name": "w", "kind": "graph", "token": "env:CB_G", "machine": "any"}, {"name": "f", "kind": "jmap", "token": "env:CB_FM"}]}))


def test_watcher_reads_mail_once_per_15_minutes_and_notifies_without_content(home, monkeypatch):
    _mail(home, monkeypatch)
    sent = []
    monkeypatch.setattr(watch, "notify", lambda text: sent.append(text))
    s = watch.run_cycle()
    assert s["added"] == 1 and results.summary().get("mail-interview") == 1
    assert any(p.source_name == "LinkedIn alert" for p in sources.load()), "alerts become postings"
    assert "1 new interview email(s). Log each stage with: careerbrew jobs log <company> <tier> <stage>" in sent
    assert all(re.fullmatch(r"(\d+ new interview email.*|careerbrew has \d+ proposal\(s\) and \d+ question\(s\) for you\. Open: careerbrew)", t) for t in sent), "counts only, no titles"
    calls = []
    monkeypatch.setattr("careerbrew.mail.fetch", lambda days=1: calls.append(days) or ([], []))
    watch.run_cycle()
    assert calls == [], "mail is throttled to every 15 minutes"


def test_corporate_watcher_keeps_no_job_mail(home, monkeypatch):
    _mail(home, monkeypatch)
    machine.set_role("corporate")
    monkeypatch.setattr(watch, "notify", lambda text: None)
    watch.run_cycle()
    assert results.summary().get("mail-interview") is None
    assert not (home / "jobs").exists()


def test_morning_plan_notification_once_per_day_counts_only(home, monkeypatch):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text(f"employer: {CANARIES[1]}\n")
    main(["work", "task", f"secret {CANARIES[1]} work", "--hours", "1"])
    sent = []
    monkeypatch.setattr(watch, "notify", lambda text: sent.append(text))
    watch._maybe_plan()
    watch._maybe_plan()
    assert len(sent) == 1 and sent[0].startswith("Today's plan is ready:") and CANARIES[1] not in sent[0]


def test_interview_gaps_lists_pipeline_companies_without_loops(ats_sources, capsys):
    sources.scan()
    assert main(["jobs", "interview-gaps"]) == 0
    out = capsys.readouterr().out
    assert "pipeline companies have a sourced interview loop" in out and "research next:" in out
