"""US-112: independent mode. Each personal repo is an income bet; `/indie` ranks them, measures outcomes
per agent-hour, and reviews weekly. Synthetic repos and numbers only."""

from __future__ import annotations

import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

import pytest

from careerbrew import cli, config, indie, machine, runs
from careerbrew.work import projects, sessions

NOW = "2026-11-19T10:00+00:00"
AMOUNT = "43217"
CANARY = "zq-indie-canary-5"
TASKS = "# Tasks\n\n## P1\n\n- [ ] A blocked one\n  - **ID**: blocked-1\n  - **Blocked**: owner step\n\n- [ ] The first open one\n  - **ID**: open-{name}\n"


def _git(path: Path, *args: str, when: str = "") -> None:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    if when:
        env |= {"GIT_AUTHOR_DATE": when, "GIT_COMMITTER_DATE": when}
    subprocess.run(["git", "-C", str(path), "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", *args], check=True, capture_output=True, env=env)


def _repo(root: Path, name: str, commits: list[tuple[str, str]]) -> Path:
    path = root / name
    path.mkdir(parents=True)
    _git(path, "init", "-q", "-b", "main")
    (path / "TASKS.md").write_text(TASKS.format(name=name), encoding="utf-8")
    _git(path, "add", "-f", "TASKS.md")
    for when, subject in commits:
        _git(path, "commit", "-q", "--allow-empty", "-m", subject, when=when)
    return path


@pytest.fixture(autouse=True)
def apps(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    cli.main(["init"])
    root = tmp_path / "apps"
    _repo(root, "alpha", [("2026-11-03T10:00+00:00", "feat: early"), ("2026-11-18T15:00+00:00", "feat: one"), ("2026-11-18T16:00+00:00", "fix: two"), ("2026-11-18T17:00+00:00", "docs: not an outcome")])
    _repo(root, "beta", [("2026-11-18T09:00+00:00", "chore: upkeep only")])
    _repo(root / "tooling", "gamma", [("2026-11-10T09:00+00:00", "perf: faster")])
    (root / "notes").mkdir()
    config.set_value("indie_roots", str(root))
    return root


def _activate(name: str, kind: str = "product") -> None:
    indie.set_bet(name, kind=kind, hypothesis=f"{CANARY} people pay for {name}", metric="weekly users", success=10, pivot=3)
    assert cli.main(["indie", "decide", name, "activate", "--yes"]) == 0


def _plan() -> dict:
    return indie.plan()


def test_scan_finds_repos_two_levels_deep_and_proposes_each_once():
    got = indie.scan()
    assert got["bets"] == ["alpha", "beta", "gamma"] and got["added"] == ["alpha", "beta", "gamma"]
    assert indie.scan()["added"] == []
    assert {b["state"] for b in indie.bets().values()} == {"proposed"}


def test_indie_repos_limits_the_bets_to_the_named_repos():
    config.set_value("indie_repos", "alpha, gamma")
    assert indie.scan()["bets"] == ["alpha", "gamma"]


def test_a_bet_with_no_hypothesis_is_a_question_for_the_owner_not_work():
    p = _plan()
    assert p["sessions"] == [] and p["questions"] == ["alpha", "beta", "gamma"]
    assert all(b["next"]["kind"] == "question" and not b["slot"] for b in p["bets"])
    assert p["coverage"] == {"repos": 3, "with_next_action": 3}


def test_only_the_owner_activates_a_bet_and_only_with_a_full_hypothesis(capsys):
    indie.scan()
    assert cli.main(["indie", "decide", "alpha", "activate"]) == 2
    assert "add --yes after the owner says so" in capsys.readouterr().out
    assert cli.main(["indie", "decide", "alpha", "activate", "--yes"]) == 2
    assert "lacks kind, hypothesis, metric, success, pivot" in capsys.readouterr().out
    _activate("alpha")
    assert indie.bets()["alpha"]["state"] == "active"
    assert "alpha" in projects._load()  # noqa: SLF001 (a session needs the project record)


def test_an_active_bet_gets_a_session_and_its_first_unblocked_task():
    indie.scan()
    _activate("alpha")
    p = _plan()
    assert p["sessions"] == ["alpha"]
    top = p["bets"][0]
    assert top["bet"] == "alpha" and top["slot"] and top["next"]["kind"] == "task"
    assert top["next"]["id"] == "open-alpha" and top["next"]["priority"] == "P1"


def test_ranking_moves_sessions_toward_the_bet_whose_work_pays_back():
    indie.scan()
    for name in ("alpha", "beta", "gamma"):
        _activate(name)
    config.set_value("work_session_cap", "2")
    assert [b["bet"] for b in indie.rank() if b["slot"]] == ["alpha", "gamma"]  # untried: most recent outcomes first
    indie.log_day("alpha", 2.0, day="2026-11-18")  # two outcomes since that day
    indie.log_day("beta", 2.0, day="2026-11-18")  # none
    rows = {b["bet"]: b for b in indie.rank()}
    assert rows["gamma"]["rank"] == 1 and rows["gamma"]["slot"]  # the untried bet is tried first
    assert rows["alpha"]["score"] > rows["beta"]["score"]
    assert rows["alpha"]["slot"] and not rows["beta"]["slot"]
    indie.log_day("gamma", 2.0, day="2026-11-18")
    assert [b["bet"] for b in indie.rank() if b["slot"]][0] == "alpha"


def test_a_bet_under_its_pivot_line_twice_is_held_until_the_owner_decides(capsys):
    indie.scan()
    _activate("alpha")
    assert cli.main(["indie", "measure", "alpha", "--value", "1"]) == 0
    assert _plan()["sessions"] == ["alpha"]  # one reading does not hold it
    assert cli.main(["indie", "measure", "alpha", "--value", "2"]) == 0
    assert "held until the owner decides" in capsys.readouterr().out
    p = _plan()
    assert p["sessions"] == [] and "alpha" in p["questions"]
    assert next(b for b in p["bets"] if b["bet"] == "alpha")["next"]["kind"] == "decide"
    assert cli.main(["indie", "decide", "alpha", "persevere", "--yes"]) == 0
    assert _plan()["sessions"] == ["alpha"]
    indie.measure("alpha", 12)
    indie.measure("alpha", 1)
    assert _plan()["sessions"] == ["alpha"]  # KEEP then PIVOT: not two in a row


def test_readings_are_keep_watch_or_pivot_and_lower_is_better_flips_them():
    indie.scan()
    _activate("alpha")
    assert [indie.measure("alpha", v)["status"] for v in (12, 5, 2)] == ["KEEP", "WATCH", "PIVOT"]
    indie.set_bet("alpha", higher_is_better=False, success=3, pivot=10)
    assert [indie.measure("alpha", v)["status"] for v in (2, 5, 12)] == ["KEEP", "WATCH", "PIVOT"]


def test_a_measure_command_runs_in_the_repo_without_a_shell(apps):
    indie.scan()
    _activate("alpha")
    (apps / "alpha" / "value.txt").write_text("7\n", encoding="utf-8")
    indie.set_bet("alpha", measure=f"{sys.executable} -c \"print('users'); print(open('value.txt').read())\"")
    assert indie.measure("alpha")["value"] == 7.0
    indie.set_bet("alpha", measure=f"{sys.executable} -c \"print('no number')\"")
    with pytest.raises(indie.IndieError, match="must print a number"):
        indie.measure("alpha")


def test_tool_bets_get_at_most_one_session_a_day():
    indie.scan()
    _activate("alpha", "enabler")
    _activate("gamma", "enabler")
    _activate("beta")
    rows = {b["bet"]: b for b in indie.rank()}
    assert rows["beta"]["slot"] and sum(rows[n]["slot"] for n in ("alpha", "gamma")) == 1
    assert "at most 1 tool bet a day" in next(rows[n]["reason"] for n in ("alpha", "gamma") if not rows[n]["slot"])


def test_paused_and_killed_bets_get_no_session():
    indie.scan()
    _activate("alpha")
    _activate("beta")
    indie.decide("alpha", "pause")
    indie.decide("beta", "kill")
    p = _plan()
    assert p["sessions"] == [] and "alpha" not in p["questions"]


def test_review_counts_outcomes_per_week_and_what_each_one_cost():
    indie.scan()
    indie.log_day("alpha", 3.0, asks=1, day="2026-11-18")
    got = indie.review(4)
    weeks = got["weeks"]
    assert [w["week_ending"] for w in weeks] == ["2026-10-29", "2026-11-05", "2026-11-12", "2026-11-19"]
    assert [w["outcomes"] for w in weeks] == [0, 1, 1, 2]  # feat, perf, then feat + fix; docs and chore are upkeep
    assert weeks[3]["agent_hours"] == 3.0 and weeks[3]["hours_per_outcome"] == 1.5 and weeks[3]["asks_per_outcome"] == 0.5
    assert weeks[1]["agent_hours"] is None and weeks[1]["hours_per_outcome"] is None  # missing hours are not zero
    assert got["trend"]["outcomes"] == {"earlier": 0.5, "recent": 1.5}
    assert {b["bet"]: b["outcomes"] for b in got["bets"]} == {"alpha": 3, "beta": 0, "gamma": 1}


def test_agent_hours_come_from_the_owners_command_when_it_is_set(tmp_path):
    indie.scan()
    fake = tmp_path / "agent-time.py"
    fake.write_text("import json, sys\nproject, days = sys.argv[1], int(sys.argv[2])\nprint(json.dumps({'toolMs': days * 3_600_000 // 7 if project == 'alpha' else 0}))\n", encoding="utf-8")
    config.set_value("indie_agent_time_command", f"{sys.executable} {fake} {{project}} {{days}}")
    weeks = indie.review(2)["weeks"]
    assert [w["agent_hours"] for w in weeks] == [1.0, 1.0]
    assert weeks[1]["hours_per_outcome"] == 0.5


def test_end_records_hours_and_asks_from_the_days_sessions(monkeypatch):
    indie.scan()
    _activate("alpha")
    sessions.open_session("alpha", "sess-alpha")
    sessions.ask("sess-alpha", "Ship the synthetic change?")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-19T11:30+00:00")
    rows = indie.end()
    assert rows == [{"id": "2026-11-19:alpha", "day": "2026-11-19", "bet": "alpha", "hours": 1.5, "asks": 1, "session": "sess-alpha"}]
    assert indie.end({"alpha": 0.5})[0]["hours"] == 0.5
    assert len(indie.day_rows()) == 1  # one row per bet and day


def test_no_amount_and_no_hypothesis_leave_through_agent_output(capsys):
    indie.scan()
    _activate("alpha")
    assert cli.main(["indie", "goal", "--monthly", AMOUNT, "--by", "2027-12-31"]) == 0
    assert cli.main(["indie", "income", "--month", "2026-10", "--amount", "4321.7", "--bet", "alpha"]) == 0
    for argv in (["goal"], ["goal", "--json"], ["income", "--json"], ["bets", "--json"], ["bet", "alpha", "--json"], ["bet", "alpha"], ["run", "--json"], ["review", "--json"], ["run"], ["review"]):
        assert cli.main(["indie", *argv]) == 0
    out = capsys.readouterr().out
    assert "10.0% of the target" in out
    assert AMOUNT not in out and "4321" not in out and CANARY not in out
    page = runs.indie_day(indie.plan())
    assert AMOUNT not in json.dumps([vars(i) for i in [*page.done, *page.next, *page.blockers]]) and CANARY not in page.headline


def test_the_owner_reads_a_bets_hypothesis_in_their_own_terminal(capsys, monkeypatch):
    indie.scan()
    _activate("alpha")
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["indie", "bet", "alpha"]) == 0
    assert CANARY in capsys.readouterr().out


def test_bad_input_is_refused_with_the_story(capsys):
    indie.scan()
    assert cli.main(["indie", "bet", "nope"]) == 2
    assert cli.main(["indie", "goal", "--monthly", "-5"]) == 2
    assert cli.main(["indie", "income", "--amount", "5"]) == 2
    assert cli.main(["indie", "end", "--hours", "alpha"]) == 2
    out = capsys.readouterr().out
    assert out.count("breaks US-112") == 4


def test_indie_is_refused_on_a_corporate_machine(capsys):
    machine.set_role("corporate", "")
    assert cli.main(["indie", "run", "--json"]) == 2
    assert "personal-only" in capsys.readouterr().out


def test_the_day_page_lists_sessions_and_owner_decisions():
    indie.scan()
    _activate("alpha")
    page = runs.indie_day(indie.plan())
    assert page.counts == {"bets": 3, "with_next_action": 3, "sessions": 1, "questions": 2}
    assert any(i.id == "ask-beta" for i in page.next) and any(i.id == "goal" for i in page.next)


def test_the_ranking_beats_round_robin_in_a_seeded_eight_week_simulation():
    gain = indie.simulation_gain()
    assert gain["mean"] >= 0.30 and gain["p10"] >= 0.20 and gain["worst"] > 0, gain
    seeds = range(100, 300)
    mine = [indie.simulate("ucb", s) for s in seeds]
    control = [indie.simulate("round-robin", s) for s in seeds]

    def mean(rows, weeks, key):
        return statistics.mean(sum(r[w][key] for w in weeks) / len(weeks) for r in rows)

    assert mean(mine, (6, 7), "hours_per_outcome") < 0.85 * mean(mine, (0, 1), "hours_per_outcome")  # it gets cheaper as it learns
    assert mean(mine, (6, 7), "outcomes") > 1.15 * mean(mine, (0, 1), "outcomes")  # and delivers more
    assert mean(mine, (6, 7), "outcomes") > 1.4 * mean(control, (6, 7), "outcomes")
    assert indie.simulate("ucb", 7) == indie.simulate("ucb", 7)  # seeded: the same run every time
