"""Issue #227: a PM update with a bare `#123` or a ticket key gives the reader nothing to click, so it is refused."""

import json

import pytest

from careerbrew import config, events
from careerbrew.day import log as daylog

DAY = "2026-10-16"


def test_a_bare_pr_number_is_refused_and_the_line_is_named(home):
    text = "billing: closed the ledger (#12).\ntooling: faster sync (https://github.example.com/o/r/pull/9)."
    with pytest.raises(daylog.UnlinkedRefError) as e:
        daylog.save_pm_draft(DAY, text)
    assert e.value.lines == ["billing: closed the ledger (#12)."], "only the line with no link is named"
    assert "billing: closed the ledger (#12)." in str(e.value) and "link" in str(e.value)
    assert not daylog.pm_draft_path(DAY).exists(), "nothing is saved"


def test_a_bare_ticket_key_is_refused(home):
    with pytest.raises(daylog.UnlinkedRefError) as e:
        daylog.save_pm_draft(DAY, "billing: BILL-7 is done.")
    assert e.value.lines == ["billing: BILL-7 is done."]


@pytest.mark.parametrize(
    "line",
    [
        "billing: closed the ledger ([#12](https://github.example.com/o/r/pull/12)).",
        "billing: [BILL-7](https://tracker.example.com/browse/BILL-7) is done.",
        "billing: <https://github.example.com/o/r/pull/12|#12> merged.",
        "billing: https://tracker.example.com/browse/BILL-7 is done.",
        "shop: decode UTF-8 file names and parse ISO-8601 dates.",
        "tooling: wrote C# notes and a Slack <#C123|team> post.",
    ],
)
def test_a_linked_ref_or_a_word_shaped_like_a_key_passes(home, line):
    daylog.save_pm_draft(DAY, line)
    assert daylog.pm_draft_path(DAY).exists()


def test_ticket_keys_follow_the_project_priority_pattern(home):
    config.set_value("project_priority", "billing=BILL-*")
    daylog.save_pm_draft(DAY, "billing: the OPS-3 runbook is ready.")  # OPS is no named ticket project, so it is a word
    with pytest.raises(daylog.UnlinkedRefError):
        daylog.save_pm_draft(DAY, "billing: BILL-7 is done.")


def test_an_empty_save_still_resets(home):
    daylog.save_pm_draft(DAY, "My own words")
    daylog.save_pm_draft(DAY, "")
    assert not daylog.pm_draft_path(DAY).exists()


def test_a_refused_save_logs_a_content_free_event(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    with pytest.raises(daylog.UnlinkedRefError):
        daylog.save_pm_draft(DAY, "billing: secret-canary #12 done.")
    raw = events.log_path().read_text()
    rows = [json.loads(x) for x in raw.splitlines() if '"day.pm.draft"' in x]
    assert rows[-1]["status"] == "refused" and rows[-1]["unlinked"] == 1
    assert "secret-canary" not in raw


def test_send_refuses_a_bare_ref_too(home, monkeypatch):
    config.set_value("pm_slack", "#pm")
    with pytest.raises(daylog.UnlinkedRefError):
        daylog.send_pm("*PM update*\nbilling: BILL-7 is done.")


def test_the_generated_draft_always_passes(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    d = {"day": DAY, "tomorrow": "2026-10-19", "done": {"Shipped": ["BILL-1 export (#4)", "fix: flaky test (#5)"]}, "planned": [], "blockers": []}
    assert daylog.unlinked_refs(daylog.pm_report(d)["draft"]) == []


def test_the_web_save_answers_400_with_the_lines(home):
    from careerbrew import server

    with pytest.raises(server.ApiError) as e:
        server.post("/api/pm/draft", {"day": DAY, "text": "billing: #12 done."})
    assert e.value.status == 400 and "billing: #12 done." in str(e.value)


def test_day_send_refuses_a_bare_ref_and_names_only_the_count_to_an_agent(home, monkeypatch, capsys):
    from careerbrew import cli

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    rc = cli.main(["day", "send", "--via", "email", "--text", "billing: secret-canary #12 done."])
    out = capsys.readouterr().out
    assert rc == 1 and "1 line(s) name a PR or ticket with no link" in out
    assert "secret-canary" not in out and "mailto:" not in out, "an agent sees the count, never the line; nothing is drafted"
