"""Seed the synthetic data root for the Playwright suite in one process (task web-seed-one-process).

`web-seed-and-serve.sh` sets the environment, runs this, then serves. Each step is the same `careerbrew` command the
seed ran as its own process before, called through `cli.main`, so Python starts and imports once instead of ~30 times.
Usage: web_seed.py <fixtures folder>
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
import tempfile
from datetime import date, datetime, timedelta
from pathlib import Path


def cb(*argv: str, stdin: str = "", **env: str) -> None:
    """One CLI call, as `cb` in the shell seed: output dropped, a nonzero exit stops the seed."""
    from careerbrew import cli

    saved = {k: os.environ.get(k) for k in env}
    os.environ.update(env)
    sys.stdin = io.StringIO(stdin)
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            rc = cli.main(list(argv))
    finally:
        sys.stdin = sys.__stdin__
        for k, v in saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
    if rc:
        raise SystemExit(f"web seed: `careerbrew {' '.join(argv)}` exited {rc}")


def moved_asks(fixture: Path, day: date) -> str:
    """The work feed fixture is dated around Monday 2026-10-19. Each event moves to the pinned day and keeps its local
    time and its distance in workdays (Monday to Friday, the default work_days): Thursday 16:30 before a Monday becomes
    the second workday before the pinned day. So at the pinned 09:00 every ask waits the same work time on any day."""
    fixture_day, out = date(2026, 10, 19), []
    workday = lambda d: d.weekday() < 5  # noqa: E731
    for line in fixture.read_text(encoding="utf-8").splitlines():
        event = json.loads(line)
        at = datetime.fromisoformat(event["at"]).replace(tzinfo=None)  # local wall time; the server adds its time zone
        back, moved = sum(workday(at.date() + timedelta(k)) for k in range((fixture_day - at.date()).days)), day
        while back:
            moved -= timedelta(days=1)
            back -= workday(moved)
        event["at"] = datetime.combine(moved, at.time()).isoformat()
        out.append(json.dumps(event) + "\n")
    return "".join(out)


def main(fix: Path) -> None:
    os.environ["CAREERBREW_TEST_OWNER"] = "1"
    home = Path(os.environ["CAREERBREW_HOME"])
    cb("init")
    (home / "jobs" / "sources.yaml").write_text(
        "sources:\n"
        f"  - {{kind: greenhouse, board: northwind, company: Northwind, url: 'file://{fix}/ats/greenhouse-northwind.json'}}\n"
        f"  - {{kind: lever, board: contoso, company: Contoso, url: 'file://{fix}/ats/lever-contoso.json'}}\n"
        f"  - {{kind: ashby, board: fabrikam, company: Fabrikam, url: 'file://{fix}/ats/ashby-fabrikam.json'}}\n",
        encoding="utf-8",
    )
    (home / "day" / "calendars.yaml").write_text(f"calendars: [{{kind: ics, path: '{fix}/calendar/work.ics'}}]\n", encoding="utf-8")
    cb("jobs", "scan")
    cb("jobs", "rate", "gh:Northwind:101", "5")
    cb("jobs", "strategy")
    cb("work", "task", "Write the queue design doc", "--impact", "3", "--hours", "1.5", "--competencies", "design")
    # Task pm-why-it-matters: its expected impact gives the PM update's line an owner-only "why it matters" note.
    cb("work", "impact", "t001", "--expect", "fewer queue questions in design review", "--who", "the platform team")
    # Issue #143: a task with its own why (a URL in it) and links: a file, a labeled ticket, a channel to post in, and a
    # javascript: value that must stay text. It is the top task, so Dashboard, Today, and Week all show it.
    cb(
        "work", "task", "Fix the flaky checkout test", "--impact", "3", "--hours", "0.5", "--why", "CI failed twice: https://ci.example.com/builds/812",
        "--link", "https://code.example.com/acme/shop/src/branch/main/tests/test_checkout.py", "--link", "ticket=https://jira.example.com/browse/SHOP-12",
        "--link", "post in #checkout=https://slack.example.com/archives/C0CHECKOUT", "--link", "javascript:alert(1)",
    )  # fmt: skip
    cb("work", "task", "Migrate billing jobs", "--impact", "5", "--hours", "6", "--due", "2026-10-21")
    cb("work", "task", "Plan the Q1 platform roadmap", "--impact", "4", "--hours", "12", "--due", "2026-11-12", "--competencies", "impact")
    cb("work", "project", "import", f"{fix}/projects/team-goals.csv")  # a team goal list with parents and links (issue #69)
    cb("work", "project", "update", "billing", "team", "Sam Lee is the new tech lead")  # the latest change in Watch list (issue #172)
    cb("work", "note", "feat: shipped order export", "--tags", "delivery", "--impact", "used by 40% of merchants")
    cb("work", "note", "Led the checkout incident review", "--tags", "ownership")
    cb("prep", "story", "Raised the bar on testing", "--competencies", "quality,leadership")
    cb("prep", "check", "Stripe", "--yes", "logistics,environment")
    cb("import", "notion", "--dir", f"{fix}/notion-bundle")
    cb("day", "watch", "once")
    # Standup rows (issue #110), as a plugin passes them: yours, Sam's (a note of yours matches "checkout"), and Ana's (nothing matches).
    rows = [
        {"title": "Order export", "status": "In review", "owner": "me", "aliases": ["order export"]},
        {"title": "Checkout reliability", "status": "In progress", "owner": "Sam Lee", "aliases": ["checkout"]},
        {"title": "Search ranking", "status": "To do", "owner": "Ana"},
        {"title": "Rollout plan", "status": "In progress", "owner": "me"},
    ]
    cb("work", "standup", "--rows", "-", stdin=json.dumps(rows))
    # Issue #229: an agent's talking point and a Slack thread careerbrew does not collect, both on "Rollout plan",
    # which has no other work. The thread is an hour before the pinned clock, so it is inside the window on any day.
    hour_ago = (datetime.fromisoformat(os.environ["CAREERBREW_NOW"]) - timedelta(hours=1)).isoformat(timespec="minutes")
    extra = home / "tmp-standup-extra.json"
    extra.write_text(json.dumps([{"title": "Agreed the rollout order with support", "at": hour_ago, "kind": "slack", "url": "https://slack.example.com/archives/C0ROLLOUT/p1", "row": "Rollout plan"}]), encoding="utf-8")
    talk = {"Rollout plan": "Agreed the rollout order with support in Slack. Next: the first region goes live on Wednesday. Nothing blocks it."}
    cb("work", "standup", "--talk", "-", "--extra", str(extra), stdin=json.dumps(talk))
    extra.unlink()
    # Notifications (issue #74): opted in, a page and a DM in the feed, and an agent's task suggestion.
    # CAREERBREW_NOTIFY=off keeps them in the web app only: no pop-up during a test run.
    # Both are dated on the pinned day: an event older than 24 h is not news, so a later CAREERBREW_NOW would drop them.
    # Local time (no Z), like the work feed fixture below: in UTC a winter day would sort them before its 08:15 DM.
    day = os.environ["CAREERBREW_NOW"][:10]
    # The day before has a log too, so Today's "Earlier day" picker has a day to open.
    cb("day", "log", CAREERBREW_NOW=f"{date.fromisoformat(day) - timedelta(days=1)}T17:00")
    cb("config", "set", "notify_events", "true")
    cb("config", "set", "notify_min_priority", "3")
    feed = [
        {"id": "pagerduty:PSEED1", "source": "pagerduty", "kind": "page", "status": "triggered", "at": f"{day}T08:40:00", "title": "Page: checkout latency over SLO", "actor": "other", "to_me": True, "thread": "pagerduty:PSEED1", "url": "https://pagerduty.example.com/incidents/PSEED1"},
        {"id": "slack:D1:1760877000.000100", "source": "slack", "kind": "dm", "at": f"{day}T08:30:00", "title": "DM from Sam: can you look at the rollout?", "actor": "other", "to_me": True, "thread": "slack:D1", "url": "https://slack.example.com/archives/D1/p1760877000000100"},
    ]  # fmt: skip
    cb("work", "feed", "add", "-", stdin="".join(json.dumps(e) + "\n" for e in feed))
    cb("work", "task", "Review the rollout checklist", "--source", "agent", "--why", "the release is Thursday")
    cb("notify", "check")
    # Work feed (US-63): 2 more open asks, 1 answered, 1 personal, 1 fyi. Added after the check, so they stay out of
    # the notifications above; Today lists 4 open asks (these 2, then the page and the DM).
    cb("work", "feed", "add", "-", stdin=moved_asks(fix / "feed" / "web-asks.jsonl", date.fromisoformat(day)))
    cb("day", "plan")  # run pages for the Reports view; review's is the newest
    if os.environ.get("CAREERBREW_VISUAL_SEED") == "1":
        from careerbrew.day import watch

        # Pin the visual measurement before saved Reports data, not only the /api/review response.
        watch.write_state(cpu_history=[120])
    cb("review")
    cb("config", "set", "stt_command", f"{fix}/voice/fake-stt.sh {{audio}}")  # synthetic local speech-to-text
    # Slack is connected with a synthetic token, and the server reads recorded answers instead of the network, so
    # Today's "Send to Slack" really sends (tests/fixtures/http/slack.com). Set last: jobs scan above reads file:// URLs.
    # The shell wrapper exports the same two values for the server.
    os.environ["CB_SLACK"] = "synthetic-not-a-token"
    os.environ.setdefault("CAREERBREW_HTTP_FIXTURES", str(fix / "http"))
    cb("connect", "slack", "--ref", "env:CB_SLACK", "--no-verify")


def serve_next_step(fix: Path, day: date) -> None:
    """Own one minimal synthetic world for the Next step case."""
    with tempfile.TemporaryDirectory(prefix="careerbrew-next-step-") as folder:
        root = Path(folder)
        home = root / "home"
        home.mkdir()
        os.environ.update(
            HOME=str(home),
            XDG_CONFIG_HOME=str(home / "config"),
            XDG_CACHE_HOME=str(home / "cache"),
            XDG_STATE_HOME=str(home / "state"),
            CAREERBREW_HOME=str(root / "data"),
            CAREERBREW_KEY_FILE=str(root / "key"),
            CAREERBREW_PAIR_KEY_FILE=str(root / "pair"),
            CAREERBREW_BACKUP_DIRS=str(root / "backups"),
            CAREERBREW_COMMANDS_DIR=str(root / "commands"),
            CAREERBREW_PUBLIC_CARDS=str(root / "cards"),
            CAREERBREW_CLAUDE_CONFIG=str(home / "claude.json"),
            CAREERBREW_CODEX_CONFIG=str(home / "codex.toml"),
            CAREERBREW_NOW=f"{day}T17:16:00",
            CAREERBREW_EVENTS="off",
            CAREERBREW_LLM="off",
            CAREERBREW_NOTIFY="off",
            CAREERBREW_POSTHOG_KEY="",
            CAREERBREW_TELEMETRY_URL="http://127.0.0.1:9",
            UV_OFFLINE="1",
            UV_NO_SYNC="1",
            UV_PYTHON_DOWNLOADS="never",
        )
        from careerbrew import server
        from careerbrew.work import feed, priorities

        cb("config", "set", "work_end", "17:30")
        row = json.loads((fix / "feed" / "next-step-ask.jsonl").read_text(encoding="utf-8"))
        row["at"] = f"{day}T16:00:00"
        cb("work", "feed", "add", "-", stdin=json.dumps(row) + "\n")
        if priorities.load() or [ask["id"] for ask in feed.open_asks()] != [row["id"]]:
            raise SystemExit("Next step fixture requires exactly one open ask and no tasks")
        # Playwright owns the process. Bind before announcing the OS-selected
        # port. Use the real request handler and built app without a warmer.
        with server.make_server(0, "e2e-token-not-secret") as http:
            print(f"careerbrew next-step fixture: http://127.0.0.1:{http.server_address[1]}/", flush=True)
            try:
                http.serve_forever()
            except KeyboardInterrupt:
                pass


if __name__ == "__main__":
    if sys.argv[1:] == ["--next-step"]:
        serve_next_step(Path(__file__).resolve().parents[1] / "fixtures", date.fromisoformat(os.environ["CAREERBREW_NEXT_STEP_DAY"]))
    elif len(sys.argv) == 2:
        main(Path(sys.argv[1]))
    else:
        raise SystemExit("expected the fixtures directory or --next-step")
