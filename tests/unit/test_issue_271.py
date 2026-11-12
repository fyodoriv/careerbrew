"""Issue #271: the standup hides rows with no update, and gates rows on fresh live proofs.

Synthetic rows, evidence, and proofs only. The clock is pinned with CAREERBREW_NOW. 2026-10-19 is a Monday; the
default standup is 10:00 for 15 minutes, Monday to Friday, so a Monday 09:40 run counts from Friday's standup.
"""

from __future__ import annotations

import json

import pytest

from careerbrew import config, events, store
from careerbrew.cli import main
from careerbrew.work import evidence, standup

NOW = "2026-10-19T09:40"
ROWS = [
    {"title": "Order export", "status": "In review", "owner": "me", "keys": ["PROJ-21"]},
    {"title": "Billing ledger", "status": "Done", "owner": "me", "keys": ["PROJ-40"]},  # done before the window
    {"title": "Search ranking", "status": "In progress", "owner": "Ana", "keys": ["PROJ-50"]},
]
PR = {"id": "pr:acme/shop#12", "kind": "pr", "repo": "acme/shop", "title": "PROJ-21 add the export endpoint", "url": "https://github.example.com/acme/shop/pull/12", "at": "2026-10-16T14:00:00-04:00"}
OLD = {"id": "pr:acme/shop#9", "kind": "pr", "repo": "acme/shop", "title": "PROJ-40 close the ledger", "url": "https://github.example.com/acme/shop/pull/9", "at": "2026-10-14T14:00:00-04:00"}


def _proof(at: str = "2026-10-19T09:30", **over: object) -> dict:
    return {"source": "github", "url": "https://github.example.com/acme/shop/pull/12", "checked_at": at, "quote": "Merged on Friday", **over}


def _check(verdict: str = "pass", *proofs: dict, reason: str = "") -> dict:
    return {"verdict": verdict, "proofs": list(proofs), "reason": reason}


@pytest.fixture
def rows(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    store.write_jsonl(evidence.evidence_path(), [{"tags": [], "origin": "user", **r} for r in (PR, OLD)])
    standup.save_rows(json.dumps(ROWS))


def _titles(r: dict) -> list[str]:
    return [x["title"] for x in r["rows"]]


# ---------- 1. rows with no update are hidden ----------


def test_a_row_with_no_update_is_hidden_yours_too_and_one_done_before_the_window(rows):
    r = standup.build()
    assert _titles(r) == ["Order export"], "Billing ledger is yours but its only work was before the window"
    assert r["hidden"] == 2 and r["counts"]["hidden"] == 2
    page = standup.render(r)
    assert "2 rows with no update hidden." in page
    assert "Billing ledger" not in page and "No new work" not in page
    assert standup.hidden_line(1) == "1 row with no update hidden."


def test_an_agent_talking_point_or_extra_action_is_an_update(rows):
    standup.save_notes(talk=json.dumps({"Billing ledger": "Closed the ledger. Next: the audit."}), extra=json.dumps([{"title": "Ranking sync", "at": "2026-10-16T15:00", "kind": "meeting", "row": "Search ranking"}]))
    r = standup.build()
    assert _titles(r) == ["Order export", "Billing ledger", "Search ranking"] and r["hidden"] == 0


# ---------- 2-4. the proof gate ----------


def test_with_proofs_only_rows_that_pass_a_fresh_check_show_with_each_proof_link(rows):
    standup.save_notes(proof=json.dumps({"Order export": _check("pass", _proof()), "Search ranking": _check("pass", _proof(url="https://chat.example.com/archives/C1/p2", source="chat", quote="Ana: shipped the ranking flag"))}))
    r = standup.build()
    assert _titles(r) == ["Order export", "Search ranking"], "a passing proof is an update even with no local work"
    order = r["rows"][0]
    assert order["proofs"] == [{**_proof(), "when": "Mon Oct 19, 09:30"}]
    assert r["rows"][1]["talking_point"] == "Checked live in 1 source: see the proof below."
    assert r["counts"]["gated"] and r["counts"]["passed"] == 2 and r["counts"]["failed"] == 0
    page = standup.render(r)
    assert 'href="https://chat.example.com/archives/C1/p2"' in page and "Checked Mon Oct 19, 09:30 in chat:" in page
    assert page.count('class="proof"') == 2


@pytest.mark.parametrize(
    ("check", "reason"),
    [
        (_check("fail", _proof(), reason="the PR was reverted"), "verdict is fail"),
        (_check("pass"), "no proof"),
        (_check("pass", _proof(url="ftp://example.com/x")), "proof 1 has no http(s) url"),
        (_check("pass", _proof(source="")), "proof 1 has no source"),
        (_check("pass", _proof(quote="")), "proof 1 has no quote"),
        (_check("pass", _proof(at="soon")), "proof 1 checked_at is not an ISO time"),
        (_check("pass", _proof(at="2026-10-16T09:00")), "proof 1 was checked before the window started"),
        (_check("pass", _proof(), _proof(at="2026-10-19T08:30")), "proof 2 is older than 60 minutes"),
        (_check("pass", _proof(at="2026-10-19T11:00")), "proof 1 checked_at is in the future"),
    ],
)
def test_a_row_that_fails_the_check_stays_off_the_page_and_says_why(rows, check, reason):
    standup.save_notes(proof=json.dumps({"Order export": check}))
    r = standup.build()
    assert _titles(r) == []
    assert r["counts"]["failed"] == 1 and reason in r["failed"][0]["reasons"] and r["failed"][0]["row"] == 1
    page = standup.render(r)
    assert "Order export" not in page and reason not in page and "reverted" not in page
    assert "No row passed the proof check." in page


def test_a_row_with_work_but_no_proof_entry_fails_and_a_row_with_neither_is_hidden(rows):
    standup.save_notes(proof=json.dumps({}))
    r = standup.build()
    assert r["failed"] == [{"row": 1, "title": "Order export", "reasons": ["no proof for this row"], "reason": ""}]
    assert r["hidden"] == 2


def test_a_proof_goes_stale_as_time_passes_and_the_limit_is_a_setting(rows, monkeypatch):
    standup.save_notes(proof=json.dumps({"Order export": _check("pass", _proof())}))
    assert _titles(standup.build()) == ["Order export"]
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:31")  # still in Friday's window (previous-day), 61 minutes later
    assert _titles(standup.build()) == []
    config.set_value("standup_proof_minutes", "120")
    assert _titles(standup.build()) == ["Order export"]
    with pytest.raises(ValueError, match="whole number"):
        config.set_value("standup_proof_minutes", "0")


@pytest.mark.parametrize(
    ("proof", "error"),
    [
        (["a list"], "a mapping of row title"),
        ({"Nope": _check("pass", _proof())}, "Nope"),
        ({"Order export": "pass"}, "a check is a mapping"),
        ({"Order export": {"verdict": "maybe", "proofs": []}}, "verdict is pass or fail"),
        ({"Order export": {"verdict": "pass", "proofs": "a link"}}, "proofs is a list"),
    ],
)
def test_a_bad_proof_file_is_refused_names_the_fix_and_writes_nothing(rows, proof, error):
    with pytest.raises(standup.StandupError, match=error):
        standup.save_notes(proof=json.dumps(proof))
    assert not standup.notes_path().exists()


def test_proofs_are_kept_encrypted_for_this_window_only(rows, monkeypatch):
    standup.save_notes(proof=json.dumps({"Order export": _check("pass", _proof(quote="PROJECTZEBRA merged"))}))
    assert b"PROJECTZEBRA" not in standup.notes_path().read_bytes()
    standup.save_notes(talk=json.dumps({"Order export": "Shipped it."}))
    assert standup.load_notes()["proof"] is not None, "passing talk keeps the proofs of this window"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T11:00")
    assert standup.load_notes()["proof"] is None and standup.build()["counts"]["gated"] is False
    standup.run()
    assert not standup.notes_path().exists()


# ---------- the issue comment: clear the rows, and say when they were updated ----------


def test_an_empty_row_list_clears_the_rows_and_the_page_says_no_row_passed(rows):
    standup.save_rows("[]")
    r = standup.build()
    assert r["rows"] == [] and r["has_rows"] is False and r["cleared"] is True
    page = standup.render(r)
    assert "No row passed the proof check." in page and "No rows to report on yet" not in page
    standup.save_notes(talk="{}", extra="[]", proof="{}")  # a plugin's scheduled publish with nothing to say
    with pytest.raises(standup.StandupError, match="no rows"):
        standup.save_rows("")


def test_the_report_says_when_the_rows_were_last_passed(rows, monkeypatch):
    r = standup.build()
    assert r["updated"] == "updated 09:40" and "· updated 09:40</p>" in standup.render(r)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00")
    assert standup.build()["updated"] == "updated Mon Oct 19, 09:40", "another day names the day"


# ---------- the command and the view ----------


def test_cli_prints_failed_rows_with_reasons_but_titles_only_with_show(rows, monkeypatch, capsys, tmp_path):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    (tmp_path / "proof.json").write_text(json.dumps({"Order export": _check("fail", _proof(), reason="PROJECTOKAPI reverted"), "Search ranking": _check("pass", _proof())}))
    assert main(["work", "standup", "--proof", str(tmp_path / "proof.json")]) == 0
    out = capsys.readouterr().out
    assert "agent notes: 0 talking points, 0 extra actions, 2 proof checks (kept until the next standup)" in out
    assert "rows: 1 shown (0 yours, 1 matched by your work), 1 with no update hidden, updated 09:40" in out
    assert "proof check: 1 passed, 1 failed (failed rows stay off the page; standup_proof_minutes 60)" in out
    assert "  failed row 1: verdict is fail" in out
    assert "Order export" not in out and "PROJECTOKAPI" not in out, "titles and the agent's reason stay off a piped stdout"
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)
    assert main(["work", "standup", "--show"]) == 0
    assert "  failed row 1 Order export: verdict is fail (agent: PROJECTOKAPI reverted)" in capsys.readouterr().out
    log = events.log_path().read_text(encoding="utf-8")
    assert "PROJECTOKAPI" not in log and '"gated": true' in log and '"failed": 1' in log


def test_cli_clears_the_rows_with_an_empty_list(rows, capsys, tmp_path):
    (tmp_path / "rows.json").write_text("[]")
    assert main(["work", "standup", "--rows", str(tmp_path / "rows.json")]) == 0
    out = capsys.readouterr().out
    assert "no row passed the proof check (updated 09:40)" in out and "no rows to report on yet" not in out


def test_the_web_view_never_carries_failed_rows(rows):
    from careerbrew import server

    standup.save_notes(proof=json.dumps({"Order export": _check("fail", _proof(), reason="PROJECTOKAPI reverted")}))
    v = server.standup_view()
    assert "failed" not in v and v["counts"]["failed"] == 1 and v["updated"] == "updated 09:40"
    assert "PROJECTOKAPI" not in json.dumps(v) and "Order export" not in json.dumps(v["rows"])
