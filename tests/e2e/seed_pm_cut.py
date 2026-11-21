"""Seed the isolated US-59 editor regression using real encrypted PM writes."""

import json
import os
from datetime import date
from pathlib import Path

from careerbrew import paths, server, store
from careerbrew.day import log, pm_learn, results
from careerbrew.work import priorities, projects


def main():
    world = Path(os.environ["CAREERBREW_PM_CUT_WORLD"])
    assert world.is_absolute() and world.resolve() == world and world.name == "pm-cut-browser"
    assert world.stat().st_mode & 0o777 == 0o700
    assert (world / ".owner").read_bytes() == b"US-59 isolated PM cut fixture\n"
    assert paths.data_root() == world / "data"
    assert os.environ["CAREERBREW_NOW"] == "2026-11-02T15:00:00-05:00"
    day = date(2026, 11, 2)
    store.write_yaml(
        paths.data_root() / "settings.yaml",
        {
            "timezone": "America/New_York",
            "first_run_checklist": False,
            "notify_events": False,
        },
    )
    results.append(
        [
            {
                "id": "synthetic-373-review",
                "at": "2026-11-02T14:00:00-05:00",
                "kind": "github",
                "title": "Reviewed 4 pull requests",
                "source": "synthetic",
                "repo": "",
                "origin": "user",
            }
        ],
        day,
    )
    task = priorities.add("reconcile amber invoices", 3, 3, 0.05, [], source="user")
    projects.add("billing")
    projects.link_task("billing", task["id"])
    built = log.build(day)
    raw = "\n".join(log.pm_lines(built, learn=False))
    kept = "billing: next: reconcile amber invoices."
    assert raw == kept + "\nOther: reviewed 4 PRs."
    now = os.environ["CAREERBREW_NOW"]
    try:
        for report_day in ("2026-10-26", "2026-10-27", "2026-10-28"):
            os.environ["CAREERBREW_NOW"] = report_day + "T15:00:00-04:00"
            pm_learn.remember(report_day, raw, raw)
            pm_learn.record(report_day, kept)
    finally:
        os.environ["CAREERBREW_NOW"] = now
    pm = log.pm_report(built)
    assert pm["body"] == kept and "Other" in pm["left_out"]
    server.daylog_view()
    server.today()
    server.daylog_view()
    with (world / "seed.json").open("x", encoding="utf-8") as stream:
        json.dump({"raw": raw, "kept": kept}, stream)


if __name__ == "__main__":
    main()
