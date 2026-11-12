"""Measure the README's claims and write its Proof section.

  scripts/proof.py collect   measure now; write docs/proof/metrics.json (keeps a history per metric)
  scripts/proof.py render    write the README Proof section from docs/proof/metrics.json
  scripts/proof.py check     exit 1 when the README section differs, or a metric lacks date, kind, or command

`scripts/metrics.sh proof` runs collect, then render. Every value is a count or a
rate from repo files: never anything from private/.
"""

from __future__ import annotations

import json
import os
import platform
import re
import subprocess
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
METRICS = ROOT / "docs/proof/metrics.json"
FIELD = os.environ.get("PROOF_FIELD") == "1"  # copy this machine's real-use totals into the proof file (your own copy only)
README = ROOT / "README.md"
START, END = "<!-- proof:start -->", "<!-- proof:end -->"
REQUIRED = ("value", "unit", "date", "kind", "command")

# Each README claim, the e2e stories that prove it end to end, and why it should work.
CLAIMS = [
    (
        "Find your next job",
        "jobs",
        ["us-02-jobs-scan-filter", "us-03-want-rate-learn", "us-04-strategy", "us-19-job-platforms", "us-20-linkedin-import", "us-48-job-choice"],
        "Design rationale, no citation: a score learned from your own ratings should rank postings better than fixed filters.",
    ),
    (
        "Prepare for interviews",
        "interviews",
        ["us-05-drill", "us-06-progress-chart", "us-17-interview", "us-18-prep", "us-39-flashcards", "us-45-teacher", "us-46-exercises", "us-57-voice-practice"],
        "Spaced, tested recall beats re-reading (Cepeda et al. 2006; Roediger & Karpicke 2006).",
    ),
    ("Get promoted", "promotion", ["us-07-evidence", "us-09-promo", "us-16-linkedin", "us-28-ladder-import"], "Design rationale, no citation: evidence written down when it happens is not lost by review time."),
    (
        "Run your workday",
        "workday",
        ["us-23-day-watch", "us-25-day-plan", "us-52-daily-log", "us-59-pm-update", "us-61-work-feed", "us-63-today-open-asks", "us-82-active-now"],
        "Motivated by the cost of interruptions (Mark, Gudith & Klocke 2008): one ranked list means fewer places to check.",
    ),
    ("Give a standup in one minute", "standup", ["us-65-standup-report"], "Design rationale, no citation: a report built from the record needs no recall from memory."),
    ("Learn a codebase or a topic", "learning", ["us-56-project-explain", "us-80-learn-project", "us-83-learn-replay-tour"], "Retests spaced over days keep what you learn (Cepeda et al. 2006)."),
    ("Keep your career data private", "privacy", ["us-08-backup", "us-13-privacy", "us-43-encryption-at-rest"], "Motivated by local-first software (Kleppmann et al. 2019): data kept on your machine is not on a server."),
    (
        "One command, three ways; run pages; notifications; decisions that teach it",
        "platform",
        ["us-53-actions-and-slack", "us-67-run-pages", "us-62-notifications", "us-72-every-control", "us-84-decision-signals"],
        "Design rationale, no citation: one action list generated from the CLI cannot drift between CLI, agent, and web app.",
    ),
]


def _run(cmd: list[str]) -> str:
    out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=False)
    return out.stdout + out.stderr


def _machine() -> str:
    system = {"Darwin": "macOS", "Linux": "Linux"}.get(platform.system(), platform.system())
    return f"{system} {platform.machine()}"


def _events(paths: list[Path]) -> list[dict]:
    rows = []
    for path in paths:
        if not path.is_file():
            continue
        for line in path.read_text().splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if row.get("event") == "check.step":
                rows.append(row)
    return rows


def measure(today: str, events: list[Path]) -> dict[str, dict]:
    """Measure every metric now. Values are counts and rates only."""
    kind = f"local run, {today}, {_machine()}"
    m: dict[str, dict] = {}

    saved = ROOT / ".metrics/e2e-results.json"
    results = json.loads(saved.read_text())["results"] if saved.is_file() else {}
    # The story results date from the last scripts/e2e.sh run, not from today.
    e2e_day = date.fromtimestamp(saved.stat().st_mtime).isoformat() if saved.is_file() else today
    e2e_kind = f"local run, {e2e_day}, {_machine()}, last `scripts/e2e.sh`"
    for _claim, key, stories, _theory in CLAIMS:
        ok = sum(results.get(f"tests/e2e/{s}.sh") == "ok" for s in stories)
        m[f"stories_{key}"] = {"value": f"{ok}/{len(stories)}", "unit": "e2e stories pass", "kind": e2e_kind, "command": "scripts/e2e.sh (results in .metrics/e2e-results.json)"}

    rows = re.findall(r"^\| \d+ \|.*?(tests/e2e/us-[\w.-]+\.sh)", (ROOT / "docs/metrics.md").read_text(), re.M)
    ok = sum(results.get(r) == "ok" for r in rows)
    m["capability_coverage"] = {"value": f"{ok}/{len(rows)}", "unit": "capabilities with a passing e2e test", "kind": e2e_kind, "command": "scripts/e2e.sh results for the docs/metrics.md rows"}
    story_ok = sum(v == "ok" for v in results.values())
    m["e2e_stories"] = {"value": f"{story_ok}/{len(results)}", "unit": "e2e story files pass", "kind": e2e_kind, "command": "scripts/e2e.sh"}

    n = re.search(r"(\d+) tests? collected", _run([sys.executable, "-m", "pytest", "--collect-only", "-q", "-p", "no:xdist", "tests/unit"]))
    m["unit_tests"] = {"value": int(n.group(1)) if n else None, "unit": "unit tests", "kind": kind, "command": "uv run pytest --collect-only -q tests/unit"}

    ev = re.search(r"(\d+) CLI cases run, \d+ LLM-judged cases listed, (\d+) failure", _run([sys.executable, "evals/run.py"]))
    if ev:
        cases, fails = int(ev.group(1)), int(ev.group(2))
        m["eval_cases"] = {"value": f"{cases - fails}/{cases}", "unit": "deterministic eval cases pass", "kind": kind, "command": "evals/run.sh"}
    ag = re.search(r"(\d+)/(\d+) labels", _run([sys.executable, "evals/run.py", "agreement"]))
    m["judge_labels"] = {"value": f"{ag.group(1)}/{ag.group(2)}" if ag else None, "unit": "LLM-judged cases with an owner label", "kind": kind, "command": "evals/run.sh agreement"}

    # Field data: one machine's real-use totals from its own event log, through the CLI (local only).
    # Off by default, so the committed file holds no machine's real use; PROOF_FIELD=1 adds yours to your own copy.
    raw = _run([sys.executable, "-m", "careerbrew", "metrics", "field", "--json"]) if FIELD else ""
    try:
        fd = json.loads(raw[raw.index("{") :])
    except ValueError:
        fd = None
    if fd is not None:
        field_kind = f"real use, local aggregate, n=1 machine, since {fd['since']}"  # the machine label stays local
        has = fd["runs"] > 0
        m["field_machine"] = {"value": "one machine", "unit": "machines measured", "kind": field_kind, "command": "careerbrew metrics field --json", "since": fd["since"]}
        m["field_time_saved"] = {
            "value": fd["minutes_saved"] if fd["action_runs"] else None,
            "unit": f"min saved over {fd['action_runs']} action run(s); baselines {fd['measured_baselines']}/{fd['baselines']} timed by hand, the rest estimates",
            "kind": field_kind,
            "command": "careerbrew metrics field --json",
            "by_week": fd["minutes_saved_by_week"],
        }
        m["field_success_rate"] = {"value": fd["success_rate"] if has else None, "unit": f"% of {fd['runs']} real runs ended ok", "kind": field_kind, "command": "careerbrew metrics field --json"}
        m["field_actions_done"] = {
            "value": fd["action_runs"] if fd["action_runs"] else None,
            "unit": "manual-work actions careerbrew did (PM update, standup, day log, day plan, evidence, promotion doc, job scan)",
            "kind": field_kind,
            "command": "careerbrew metrics field --json",
        }
        m["field_errors_per_run"] = {"value": round(fd["runs_failed"] / fd["runs"], 3) if has else None, "unit": f"errors per real run ({fd['runs_failed']} of {fd['runs']})", "kind": field_kind, "command": "careerbrew metrics field --json"}

    leak = re.search(r"privacy scan: (\d+) file\(s\), (\d+) finding", _run(["scripts/leak-check.sh"]))
    if leak:
        m["leak_findings"] = {"value": int(leak.group(2)), "unit": f"findings by the leak-check patterns in {leak.group(1)} tracked files", "kind": kind, "command": "scripts/leak-check.sh"}

    steps = _events(events)
    if steps:
        days = sorted({r["ts"][:10] for r in steps})
        span = f"local runs, {days[0]} to {days[-1]}, {_machine()}"
        good = sum(bool(r.get("ok")) for r in steps)
        m["gate_step_pass_rate"] = {
            "value": round(100 * good / len(steps), 1),
            "unit": "% of gate steps passed",
            "kind": span,
            "command": "scripts/check.sh (steps in .metrics/events.jsonl)",
            "runs": len(steps),
            "by_day": {d: round(100 * sum(bool(r.get("ok")) for r in steps if r["ts"].startswith(d)) / sum(r["ts"].startswith(d) for r in steps), 1) for d in days},
        }
        e2e = [r for r in steps if r.get("name") == "e2e" and r.get("ok")]
        if e2e:
            m["e2e_seconds"] = {"value": round(sum(float(r["seconds"]) for r in e2e) / len(e2e)), "unit": "s, mean green e2e step", "kind": span, "command": "mean green `e2e` step in .metrics/events.jsonl", "runs": len(e2e)}
    for value in m.values():
        value["date"] = today
    return m


def collect(today: str | None = None, events: list[Path] | None = None) -> dict:
    today = today or date.today().isoformat()
    paths = events or [ROOT / ".metrics/events.jsonl"]
    paths += [Path(p) for p in os.environ.get("PROOF_EVENTS", "").split(":") if p]
    old = json.loads(METRICS.read_text()) if METRICS.is_file() else {"metrics": {}}
    data = {"metrics": {}}
    measured = measure(today, paths)
    if FIELD:
        started = old["metrics"].get("field_recording_since", {}).get("value") or today
        measured["field_recording_since"] = {"value": started, "unit": "date field recording turned on", "kind": f"real use, {_machine()}", "command": "careerbrew metrics field --json", "date": today}
    for name, value in measured.items():
        history = [h for h in old["metrics"].get(name, {}).get("history", []) if h["date"] != today]
        value["history"] = [*history, {"date": today, "value": value["value"]}]
        data["metrics"][name] = value
    METRICS.parent.mkdir(parents=True, exist_ok=True)
    METRICS.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    return data


def problems(data: dict) -> list[str]:
    return [f"{name}: no {field}" for name, value in data["metrics"].items() for field in REQUIRED if value.get(field) in (None, "") and not (field == "value" and name.startswith("field_"))]


FIELD_ROWS = [
    (
        "field_time_saved",
        "It saves you time (headline)",
        "Time saved = your manual minutes minus your time in careerbrew, per action (Goal-Question-Metric, Basili, Caldiera & Rombach 1994). In the web app it is your own time from the open to Copy, Send, or Accept; a CLI run counts only its run time, an upper bound. An untimed baseline is an estimate.",
    ),
    ("field_actions_done", "It does the manual work for you", "Each run of a manual-work action is work you did not do by hand."),
    ("field_success_rate", "Real runs work", "Share of real CLI runs that ended ok."),
    ("field_errors_per_run", "Few errors in real use", "Errors are counted by class only; a broken story is fixed first (AGENTS.md rule 8)."),
]


def section(data: dict) -> str:
    m = data["metrics"]
    since = m.get("field_recording_since", {}).get("value", "today")
    published = "field_recording_since" in m  # False: this copy holds no machine's real-use totals

    def cell(name: str) -> tuple[str, str]:
        v = m.get(name)
        if not published:
            return "your own number: run `careerbrew metrics field`", "—"
        if not v or v.get("value") is None:
            return f"not measured yet (field recording on since {since})", "—"
        return f"{v['value']} {v['unit']}", f"`docs/proof/metrics.json` ({v['kind']})"

    intro = [
        "Headline rows are field data from careerbrew's own content-free event log. `careerbrew metrics field` builds one machine's totals; `scripts/metrics.sh proof` copies the totals, never the machine label, into `docs/proof/metrics.json` and rewrites this section.",
        "",
        "Machines so far: one. Source: the local aggregate, n=1 machine. More machines are `not measured yet`: sending totals to PostHog through one telemetry adapter comes in a later PR; today nothing is sent from a machine.",
    ]
    if not published:
        intro = [
            "Headline rows are field data: each machine measures its own real use in its own content-free event log, and `careerbrew metrics field` shows that machine's totals.",
            "",
            "This repository holds no machine's totals, so the Result column points you to your own. `PROOF_FIELD=1 scripts/metrics.sh proof` copies your totals, never a machine label, into your copy of `docs/proof/metrics.json`; nothing is sent from a machine.",
        ]
    lines = [
        *intro,
        "",
        "| Claim | Theory | Metric | Result | Source |",
        "|---|---|---|---|---|",
    ]
    for name, claim, theory in FIELD_ROWS:
        result, source = cell(name)
        lines.append(f"| {claim} | {theory} | `careerbrew metrics field` | {result} | {source} |")
    lines.append("| Outcomes in real use (offers, promotion coverage, readiness) | Goal-Question-Metric (Basili, Caldiera & Rombach 1994) | `careerbrew review` over 4 weeks | not measured yet (task `proof-real-use-outcomes`) | — |")

    by_week = m.get("field_time_saved", {}).get("by_week", {})
    if by_week and m["field_time_saved"].get("value") is not None:
        weeks = list(by_week)
        lines += [
            "",
            f"Minutes saved per week ({m['field_time_saved']['kind']}):",
            "",
            "```mermaid",
            "xychart-beta",
            '  title "Minutes saved per week"',
            "  x-axis [" + ", ".join(f'"{w}"' for w in weeks) + "]",
            '  y-axis "minutes"',
            "  bar [" + ", ".join(str(by_week[w]) for w in weeks) + "]",
            "```",
        ]
    else:
        lines += [
            "",
            f"Minutes saved per week: no chart yet. Field recording is on since {since}; the chart appears after the first real week." if published else "Minutes saved per week: `careerbrew metrics time-saved` shows your own chart data.",
        ]

    def val(name: str) -> str:
        v = m.get(name, {})
        return "not measured yet" if v.get("value") is None else f"{v['value']} {v['unit']}"

    stories = ", ".join(f"{key} {m.get(f'stories_{key}', {}).get('value', '?')}" for _claim, key, _s, _t in CLAIMS)
    kind = m.get("e2e_stories", {}).get("kind", "local run")
    lines += [
        "",
        f"**Engineering health** (tests, not proof of value; {kind}, synthetic data):",
        "",
        f"- E2E stories per use case: {stories}.",
        f"- All stories: {val('e2e_stories')}. Capabilities: {val('capability_coverage')}.",
        f"- Unit tests: {val('unit_tests')}. Evals: {val('eval_cases')}. Judge labels: {val('judge_labels')}.",
        f"- Leak scan: {val('leak_findings')}. E2E step time: {val('e2e_seconds')}.",
    ]
    by_day = m.get("gate_step_pass_rate", {}).get("by_day", {})
    if by_day:
        days = list(by_day)
        lines += [
            f"- Gate steps passed: {val('gate_step_pass_rate')} ({m['gate_step_pass_rate']['kind']}).",
            "",
            "```mermaid",
            "xychart-beta",
            '  title "Gate steps passed, days with runs (%)"',
            "  x-axis [" + ", ".join(f'"{d[5:]}"' for d in days) + "]",
            '  y-axis "%" 0 --> 100',
            "  bar [" + ", ".join(str(by_day[d]) for d in days) + "]",
            "```",
        ]
    return "\n".join([START, *lines, END])


def render() -> None:
    text = README.read_text()
    new = section(json.loads(METRICS.read_text()))
    README.write_text(re.sub(re.escape(START) + ".*?" + re.escape(END), lambda _m: new, text, flags=re.S))


def check() -> list[str]:
    data = json.loads(METRICS.read_text())
    found = problems(data)
    text = README.read_text()
    block = re.search(re.escape(START) + ".*?" + re.escape(END), text, re.S)
    if not block:
        found.append("README.md: no proof section markers")
    elif block.group(0) != section(data):
        found.append("README.md: the Proof section differs from `scripts/proof.py render`")
    return found


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "check"
    if cmd == "collect":
        collect()
    elif cmd == "render":
        render()
    else:
        issues = check()
        for issue in issues:
            print(f"proof: {issue}")
        print(f"proof: {len(issues)} problem(s)")
        sys.exit(1 if issues else 0)
