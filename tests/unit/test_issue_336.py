"""PM checks use facts and each owner edit yields a rule candidate."""

from careerbrew import cli, store
from careerbrew.day import log, pm_learn


def sample():
    return {"day": "2026-10-30", "done": {"Shipped": ["Build impact dashboard"]}, "details": {"done": {"Shipped": [{"kind": "pr", "title": "Build impact dashboard", "draft": True, "repo": "example/shop"}]}}}


def test_no_action_checks_are_reported_and_not_misread_as_results():
    assert "no-action" in {c["rule"] for c in log.content_checks("billing: all tickets closed, no action needed.", {"done": {}})}
    assert "no-action" not in {c["rule"] for c in log.content_checks("billing: fixed a crash that needed no user action.", {"done": {}})}


def test_draft_state_check_requires_matching_source_facts():
    d = sample()
    assert "draft-state" in {c["rule"] for c in log.content_checks("shop: Build impact dashboard.", d)}
    assert "draft-state" not in {c["rule"] for c in log.content_checks("shop: Build impact dashboard. In draft", d)}
    assert "draft-state" not in {c["rule"] for c in log.content_checks("shop: Shipped invoice export.", d)}


def test_generated_draft_preserves_explicit_unfinished_state():
    assert "In draft" in "\n".join(log.pm_lines(sample(), learn=False))


def test_generated_draft_drops_no_action_checks():
    d = {"day": "2026-10-30", "done": {"Other work": ["All tickets closed, no action needed"]}, "details": {}}
    assert "no action needed" not in "\n".join(log.pm_lines(d, learn=False)).casefold()


def test_each_shown_to_saved_line_change_has_a_rule_without_self_confirmation():
    rows = [{"day": "2026-10-30", "draft": "Old raw line", "shown": "billing: Build dashboard.\nops: all closed, no action needed", "body": "billing: Build dashboard. In draft\nhiring: Shared interview tool"}]
    got = pm_learn.rules(rows)["edit_rules"]
    assert len(got) == 2
    assert all(r["rule"] and r["before"] and r["after"] for r in got)
    assert any("draft" in r["rule"] for r in got)
    assert not pm_learn.rules([{"day": "2026-10-30", "draft": "raw before learning", "shown": "Owner's unchanged shown line", "body": "Owner's unchanged shown line"}])["edit_rules"]
    assert pm_learn.MIN_TIMES == 2


def test_added_and_removed_lines_both_get_rule_candidates():
    rows = [{"day": "2026-10-30", "shown": "a: keep\nb: remove", "body": "a: keep"}, {"day": "2026-10-31", "shown": "a: keep", "body": "a: keep\nc: add"}]
    got = pm_learn.rules(rows)["edit_rules"]
    assert {r["change"] for r in got} == {"remove", "add"}


def test_restricted_send_names_checks_without_leaking_lines(monkeypatch, capsys, home):
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    store.write_text(log.pm_draft_path("2026-10-30"), "private-canary: no action needed.")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T12:00:00Z")
    cli.main(["day", "send"])
    out = capsys.readouterr().out
    assert "no-action" in out and "private-canary" not in out


def test_a_real_result_that_removes_manual_action_is_not_dropped_or_flagged():
    title = "Fixed recovery so no action is required from the owner"
    d = {"day": "2026-10-30", "done": {"Other work": [title]}, "details": {}}
    assert "no-action" not in {c["rule"] for c in log.content_checks("billing: " + title, d)}
    assert "fixed recovery" in "\n".join(log.pm_lines(d, learn=False)).casefold()


def test_send_checks_the_explicit_text_override_without_exporting_it(monkeypatch, capsys):
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["day", "send", "--text", "private-canary: no action needed."]) == 2
    out = capsys.readouterr().out
    assert "no-action" in out and "private-canary" not in out


def test_result_with_colon_or_commit_prefix_is_not_a_no_action_status():
    for title in ("Fixed recovery: no action is required from the owner", "fix: no action is required from the owner on restart"):
        d = {"day": "2026-10-30", "done": {"Other work": [title]}, "details": {}}
        assert "no-action" not in {c["rule"] for c in log.content_checks(title, d)}
        assert "no action" in "\n".join(log.pm_lines(d, learn=False)).casefold()


def test_per_edit_rule_candidates_are_persisted_inside_existing_encrypted_history(home, monkeypatch, capsys):
    day = "2026-10-30"
    pm_learn.remember(day, "private-canary: Build dashboard", "private-canary: Build dashboard")
    pm_learn.record(day, "private-canary: Build dashboard. In draft")
    row = store.read_jsonl(pm_learn.path())[0]
    assert row["edit_rules"][0]["rule"]
    assert b"private-canary" not in pm_learn.path().read_bytes()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["data", "show", "day/pm-edits.jsonl"]) == 3
    assert "private-canary" not in capsys.readouterr().out
    assert cli.main(["data", "show", "day/pm-edits.jsonl", "--keys"]) == 0
    assert "private-canary" not in capsys.readouterr().out


def test_draft_metadata_survives_gh_evidence_results_and_pm_build(home, monkeypatch):
    import json
    import subprocess
    from datetime import date

    from careerbrew.day import results, watch
    from careerbrew.work import evidence

    day = date(2026, 10, 30)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T12:00:00Z")
    store.write_yaml(home / "work" / "sources.yaml", {"sources": [{"kind": "gh", "author": "synthetic"}]})
    draft = {"number": 41, "title": "Build impact dashboard", "url": "https://github.com/example/shop/pull/41", "createdAt": "2026-10-30T09:00:00Z", "isDraft": True, "state": "open", "repository": {"nameWithOwner": "example/shop"}}
    calls = []

    def gh(argv, **kwargs):
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0, json.dumps([draft]), "")

    monkeypatch.setattr(evidence.subprocess, "run", gh)
    rows = watch._src_evidence(day)
    assert "isDraft" in calls[0][calls[0].index("--json") + 1].split(",")
    results.append(rows, day)
    assert "In draft" in log.pm_report(log.build(day))["draft"]
    # A state-only update must replace the same day's result despite an unchanged title.
    draft["isDraft"] = False
    results.append(watch._src_evidence(day), day)
    changed = log.pm_report(log.build(day))["draft"]
    assert "In draft" not in changed and "Open PR" in changed


def test_github_api_draft_metadata_reaches_pm_and_closed_is_not_merge_proof(home, monkeypatch):
    from datetime import date

    from careerbrew import connectors
    from careerbrew.day import results, watch

    day = date(2026, 10, 30)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-30T12:00:00Z")
    monkeypatch.setenv("CB_T", "synthetic")
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "user": "synthetic", "token": "env:CB_T"}]})
    row = {"id": 41, "number": 41, "title": "Build impact dashboard", "html_url": "https://github.com/example/shop/pull/41", "created_at": "2026-10-30T09:00:00Z", "draft": True, "state": "open", "pull_request": {"merged_at": None}}
    monkeypatch.setattr(connectors, "http_json", lambda *args: {"items": [row]})
    results.append(watch._src_evidence(day), day)
    assert "In draft" in log.pm_report(log.build(day))["draft"]
    row.update(draft=False, state="closed")
    items = connectors.github({"user": "synthetic", "token": "env:CB_T"}, "user")
    assert "delivery" not in items[0]["tags"]
    results.append(watch._src_evidence(day), day)
    assert "merge not confirmed" in log.pm_report(log.build(day))["draft"]
    row["pull_request"]["merged_at"] = "2026-10-30T11:00:00Z"
    results.append(watch._src_evidence(day), day)
    assert "merge not confirmed" not in log.pm_report(log.build(day))["draft"]


def test_enriching_a_real_result_never_teaches_a_drop_status_rule():
    shown = "billing: Fixed recovery so no action required from the owner"
    got = pm_learn.rules([{"day": "2026-10-30", "shown": shown, "body": shown + "; shared the runbook"}])["edit_rules"]
    assert len(got) == 1 and "drop" not in got[0]["rule"]


def test_status_page_recovery_result_is_not_mistaken_for_a_status_check():
    title = "Status page: fixed recovery; no action required from the owner"
    d = {"day": "2026-10-30", "done": {"Other work": [title]}, "details": {}}
    assert "no-action" not in {c["rule"] for c in log.content_checks("billing: " + title, d)}
    assert "fixed recovery" in "\n".join(log.pm_lines(d, learn=False)).casefold()


def test_a_draft_in_another_project_does_not_flag_this_projects_result():
    assert "draft-state" not in {c["rule"] for c in log.content_checks("billing: Build impact dashboard.", sample())}
