"""The public repo health export: allowlisted, content-free, and the same Dashboard on every machine."""

from __future__ import annotations

import json
import re
from datetime import UTC, datetime
from pathlib import Path

import pytest

from careerbrew import cli, config, paths, repo_health, report

from .conftest import CANARIES

CANARY = CANARIES[2]
NOW = datetime(2026, 10, 25, 12, 0, tzinfo=UTC)
TEMPLATE = Path(__file__).resolve().parents[2] / "config" / "repo-health-dashboard"


@pytest.fixture
def repo(tmp_path, monkeypatch):
    """A fake repo root: a gate with three steps, its events, and an e2e run whose story names hold a canary."""
    root = tmp_path / "repo"
    (root / "scripts").mkdir(parents=True)
    (root / "scripts" / "check.sh").write_text("step docs scripts/check-docs.sh\n  step unit uv run pytest\nif [ x ]; then step e2e scripts/e2e.sh; fi\n")
    (root / ".metrics").mkdir()
    ev = [
        {"ts": "2026-10-24T10:00:00Z", "event": "check.step", "name": "docs", "seconds": 1, "ok": True},
        {"ts": "2026-10-24T10:00:01Z", "event": "check.step", "name": "unit", "seconds": 40, "ok": True},
        {"ts": "2026-10-24T10:01:00Z", "event": "check.step", "name": "e2e", "seconds": 120, "ok": False},
        {"ts": "2026-10-25T09:00:00Z", "event": "check.step", "name": "docs", "seconds": 1, "ok": True},
        {"ts": "2026-10-25T09:00:01Z", "event": "check.step", "name": "unit", "seconds": 44, "ok": False},
        {"ts": "2026-10-25T09:01:00Z", "event": "check.step", "name": "e2e", "seconds": 140, "ok": True},
        {"ts": "2026-10-25T09:02:00Z", "event": "check.step", "name": CANARY.lower(), "seconds": 3, "ok": False},  # not a gate step
        {"ts": "2026-10-25T09:03:00Z", "event": "check.step", "name": "unit", "seconds": float("nan"), "ok": True},
        {"ts": "2026-08-01T09:00:00Z", "event": "check.step", "name": "unit", "seconds": 9, "ok": False},  # outside the window
        {"ts": "2026-10-25T09:04:00Z", "event": "story.fail", "story": CANARY},
    ]
    (root / ".metrics" / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in ev))
    (root / ".metrics" / "e2e-results.json").write_text(json.dumps({"results": {f"tests/e2e/{CANARY}-a.sh": "ok", "tests/e2e/b.sh": "fail"}}))
    monkeypatch.setattr(paths, "REPO_ROOT", root)
    monkeypatch.setenv("CAREERBREW_NOW", NOW.isoformat())
    return root


def test_the_export_counts_gate_steps_and_holds_no_text(repo):
    p = repo_health.collect(28)
    s = p["summary"]
    assert (s["gate_runs"], s["step_runs"], s["failed_steps"], s["e2e_passed"], s["e2e_total"]) == (2, 6, 2, 1, 2)
    assert s["checkup_status"] == "none"
    assert [r["step"] for r in p["steps"]] == ["e2e", "unit", "docs"], "highest failure rate first, then by name"
    assert p["steps"][0] == {"step": "e2e", "runs": 2, "failed": 1, "failure_rate": 0.5, "seconds_avg": 130.0}
    assert p["daily"] == [{"day": "2026-10-24", "gate_runs": 1, "step_runs": 3, "failed_steps": 1}, {"day": "2026-10-25", "gate_runs": 1, "step_runs": 3, "failed_steps": 1}]
    assert CANARY.lower() not in json.dumps(p).lower(), "a step the gate does not define, and story names, never leave"


def test_the_checkup_gives_its_status_and_counts_never_its_words(repo):
    r = report.Run("repo-checkup", "careerbrew checkup", personal=False)
    r.blockers = [report.Item(f"Gate failed {CANARY}", id="gate", why=f"why {CANARY}", command=f"echo {CANARY}", detail=CANARY)]
    r.next = [report.Item(f"Finish PR #12 ({CANARY})", id="pr-12", command="gh pr checkout 12"), report.Item("Health", id="health-1")]
    report.write(r)
    s = repo_health.collect(28)["summary"]
    assert (s["checkup_status"], s["checkup_blockers"], s["checkup_next"], s["open_prs"]) == ("blocked", 1, 2, 1)
    assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:[+-]\d{2}:\d{2}|Z)", s["checkup_at"]), "with its offset, so the time is not ambiguous"
    assert CANARY not in json.dumps(repo_health.collect(28))


@pytest.mark.parametrize(
    "change",
    [
        lambda p: p.update(extra=1),
        lambda p: p["summary"].update(checkup_status="Finish the payment rewrite"),
        lambda p: p["summary"].update(gate_runs=True),
        lambda p: p["summary"].update(gate_runs=float("inf")),
        lambda p: p["steps"].append({"step": "secret-project", "runs": 1, "failed": 0, "failure_rate": 0.0, "seconds_avg": 1.0}),
        lambda p: p["daily"].append({"day": "2026-10-25", "gate_runs": 1, "step_runs": 1, "failed_steps": 0, "note": "x"}),
        # each field takes only its own values: a slug that fits another field's pattern is still refused
        lambda p: p["summary"].update(checkup_status="acme-layoffs"),
        lambda p: p["summary"].update(checkup_at="jane-doe-offer"),
        lambda p: p["summary"].update(checkup_at="2026-10-25T09:00:00\n"),
        lambda p: p["daily"][0].update(day="acme-layoffs"),
        lambda p: p["daily"][0].update(day="2026-10-25\n"),
        lambda p: p["daily"][0].update(day="2026-13-45"),
        lambda p: p.update(version="1secretproject"),
        lambda p: p.update(generated_at="unit"),
        lambda p: p["summary"].update(gate_runs=-1),
        lambda p: p["summary"].update(failure_rate=float("nan")),
        lambda p: p["steps"][0].update(runs=[1]),
        lambda p: p["steps"][0].update(step="2026-10-25"),
    ],
)
def test_the_allowlist_refuses_anything_it_does_not_know(repo, change):
    p = repo_health.collect(28)
    change(p)
    with pytest.raises(repo_health.PublicError):
        repo_health.validate(p)


def test_a_sensitive_term_refuses_the_whole_export(repo, home):
    home.mkdir(parents=True, exist_ok=True)
    (home / "sensitive-terms.txt").write_text("unit\n")
    with pytest.raises(repo_health.PublicError, match="sensitive"):
        repo_health.collect(28)


def test_cli_prints_json_and_writes_the_dashboard_row_files(repo, tmp_path, capsys):
    out = tmp_path / "rows"
    assert cli.main(["report", "public", "--json", "--out", str(out)]) == 0
    printed = capsys.readouterr().out
    assert json.loads(printed[: printed.rindex("}") + 1])["summary"]["gate_runs"] == 2
    rows = {f.stem: json.loads(f.read_text())["rows"] for f in out.glob("*.json")}
    assert set(rows) == {"summary", "steps", "daily"}
    assert rows["summary"][0]["window_days"] == 28 and len(rows["steps"]) == 3
    assert cli.main(["report", "public", "--days", "0"]) == 1
    assert "refused" in capsys.readouterr().out


def test_the_dashboard_url_setting_takes_only_a_claude_artifact_link():
    config.set_value("checkup_artifact_url", "https://claude.ai/artifact/QqYYpXSomqh4NTps23YWog")
    assert config.get("checkup_artifact_url").endswith("QqYYpXSomqh4NTps23YWog")
    for bad in ("https://example.com/artifact/abcdefgh", "claude.ai/artifact/abcdefgh", "https://claude.ai/artifact/../x"):
        with pytest.raises(ValueError):
            config.set_value("checkup_artifact_url", bad)


def test_the_tracked_dashboard_reads_only_what_the_export_writes():
    """Every machine builds the same Dashboard from these two files, so they must match the export's columns."""
    page = (TEMPLATE / "index.html").read_text()
    spec = json.loads((TEMPLATE / "datasets.json").read_text())
    calcs = set(re.findall(r'dash\.calc\("([^"]+)"', page))
    sources = {s for attr in re.findall(r'data-source="([^"]+)"', page) for s in attr.split()}
    assert sources <= set(spec["datasets"]) | calcs
    assert set(spec["datasets"]) == {"summary", "steps", "daily"}
    summary_cols = repo_health.SUMMARY_KEYS | {"generated_at", "window_days"}
    assert set(re.findall(r'data-source="summary" data-field="([^"]+)"', page)) <= summary_cols
    assert set(re.findall(r'<th data-field="([^"]+)"', page)) <= repo_health.STEP_KEYS
    for col in re.findall(r"\br\.(\w+)", page):
        assert col in summary_cols | repo_health.STEP_KEYS | repo_health.DAY_KEYS | {"day", "failed", "runs", "rate", "step"}, col
    assert "innerHTML = r" not in page and ".html(" not in page, "data is text, never markup"


def test_a_cached_step_is_not_counted(repo):
    """A cache hit logs 0 s and ok without running; counting it would shrink every average and failure rate."""
    with (repo / ".metrics" / "events.jsonl").open("a") as fh:
        for _ in range(5):
            fh.write(json.dumps({"ts": "2026-10-25T10:00:00Z", "event": "check.step", "name": "e2e", "seconds": 0, "ok": True, "cached": True}) + "\n")
    e2e = next(r for r in repo_health.collect(28)["steps"] if r["step"] == "e2e")
    assert (e2e["runs"], e2e["failure_rate"], e2e["seconds_avg"]) == (2, 0.5, 130.0)


def test_days_are_the_owners_local_days(repo):
    """02:00 UTC on Oct 25 is 22:00 on Oct 24 in the default time zone (Etc/GMT+4)."""
    with (repo / ".metrics" / "events.jsonl").open("a") as fh:
        fh.write(json.dumps({"ts": "2026-10-25T02:00:00Z", "event": "check.step", "name": "docs", "seconds": 1, "ok": True}) + "\n")
    day = {r["day"]: r for r in repo_health.collect(28)["daily"]}
    assert day["2026-10-24"]["gate_runs"] == 2 and day["2026-10-25"]["gate_runs"] == 1


def test_a_torn_line_is_skipped_not_a_crash(repo):
    with (repo / ".metrics" / "events.jsonl").open("a") as fh:
        fh.write('{"ts": "2026-10-25T10:00:00Z", "event": "check.st')
    assert repo_health.collect(28)["summary"]["gate_runs"] == 2


def test_out_refuses_the_private_data_folder_and_a_file(repo, home, tmp_path, capsys):
    assert cli.main(["report", "public", "--out", str(home / "out")]) == 1
    assert "outside" in capsys.readouterr().out and not (home / "out" / "summary.json").exists()
    blocker = tmp_path / "a-file"
    blocker.write_text("")
    assert cli.main(["report", "public", "--out", str(blocker / "rows")]) == 1
    assert "refused" in capsys.readouterr().out


def test_a_symlinked_row_file_is_replaced_never_followed(repo, home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    private = home / "notes.md"
    private.write_text("private notes\n")
    out = tmp_path / "rows"
    out.mkdir()
    (out / "summary.json").symlink_to(private)
    repo_health.write_rows(repo_health.collect(28), out)
    assert private.read_text() == "private notes\n" and not (out / "summary.json").is_symlink()
