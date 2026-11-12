"""Sent Tomorrow bullets stay visible as next-workday promises (#331)."""

import json
from datetime import date

import pytest

from careerbrew import cli, config
from careerbrew.day import log, pm_learn
from careerbrew.work import projects, promises, sessions
from careerbrew.workday import plan

TEXT = "Today\n- Finished the prototype\n\n*Tomorrow*\n• Deliver billing export\n- Review the rollout\n- Share hiring notes\n\nRisks\n- Not a promise"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00:00-04:00")
    cli.main(["init"])
    projects.add("billing", title="Billing")
    config.set_value("project_priority", "billing=*billing*")


def test_sent_bullets_become_three_idempotent_next_workday_promises():
    pm_learn.save_sent("2026-10-30", TEXT)
    pm_learn.save_sent("2026-10-30", TEXT)
    rows = promises.open_list()
    assert len(rows) == 3
    records = projects.records()
    assert all(records[r["name"]]["promise"]["source"] == "pm-update" for r in rows)
    assert {r["due"] for r in rows} == {"2026-11-02"}
    assert {r["parent"] for r in rows} == {"billing", ""}


def test_next_plan_prioritizes_all_promises_and_marks_session_coverage():
    pm_learn.save_sent("2026-10-30", TEXT)
    sessions.open_session("billing", "billing-session")
    got = plan.build(date(2026, 11, 2))["pm_promises"]
    assert got["missing"] is False and len(got["items"]) == 3
    billing = next(r for r in got["items"] if r["project"] == "billing")
    assert billing["covered"] and billing["session_project"] == "billing"
    assert all(r["session_project"] for r in got["items"])
    assert all(r["weight"] > 0 for r in got["items"])


def test_missing_sent_update_is_explicit():
    assert plan.build(date(2026, 11, 2))["pm_promises"]["missing"] is True


def test_next_draft_names_every_promise_and_its_status():
    pm_learn.save_sent("2026-10-30", TEXT)
    row = promises.open_list()[0]
    promises.close(row["name"])
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert "Deliver billing export" in text
    assert "Review the rollout" in text
    assert "Share hiring notes" in text
    assert "done" in text and "still open" in text


def test_restricted_plan_hides_promise_text_and_project(monkeypatch, capsys):
    pm_learn.save_sent("2026-10-30", TEXT)
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    out = capsys.readouterr().out
    rows = json.loads(out)["pm_promises"]["items"]
    assert len(rows) == 3
    assert "Deliver billing" not in out and '"project": "billing"' not in out
    assert all("title" not in r and "project" not in r and "session_project" not in r for r in rows)


def test_repeat_save_keeps_project_version():
    pm_learn.save_sent("2026-10-30", TEXT)
    before = projects.path().stat().st_mtime_ns
    pm_learn.save_sent("2026-10-30", TEXT)
    assert projects.path().stat().st_mtime_ns == before


def test_public_promise_ids_are_content_free():
    pm_learn.save_sent("2026-10-30", TEXT)
    public = json.dumps(promises.open_list(business=False))
    assert "deliver billing" not in public.casefold() and "review the rollout" not in public.casefold()


def test_edited_sent_bullets_retire_removed_promises_without_reopening_done():
    pm_learn.save_sent("2026-10-30", TEXT)
    row = next(r for r in promises.open_list() if r["title"] == "Deliver billing export")
    promises.close(row["name"])
    pm_learn.save_sent("2026-10-30", TEXT.replace("- Share hiring notes\n", ""))
    assert len(promises.open_list()) == 1
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 2 and any(r["status"] == "done" for r in items)


def test_learned_style_cannot_silently_drop_prior_promises(monkeypatch):
    pm_learn.save_sent("2026-10-30", TEXT)
    monkeypatch.setattr(pm_learn, "rules", lambda: {"days": 3, "cut": {"Yesterday's promise"}, "parts": [], "rename": {}, "order": {}})
    sample = {"day": "2026-11-02", "done": {"Notes": ["Prototype work"]}, "details": {}}
    assert "Deliver billing export" in "\n".join(log.pm_lines(sample))
    assert "Share hiring notes" in log.pm_report(sample)["draft"]


def test_concurrent_sent_saves_serialize_source_and_promises(monkeypatch):
    import threading

    from careerbrew.day import pm_promises

    entered = threading.Event()
    release = threading.Event()
    second_record = threading.Event()
    errors = []
    original = pm_promises.record

    def intercept(day, text):
        if "First task" in text:
            entered.set()
            assert release.wait(5)
        else:
            second_record.set()
        return original(day, text)

    monkeypatch.setattr(pm_promises, "record", intercept)

    def save(text):
        try:
            pm_learn.save_sent("2026-10-30", text)
        except BaseException as exc:
            errors.append(exc)

    first = threading.Thread(target=save, args=("Tomorrow\n- First task",))
    second = threading.Thread(target=save, args=("Tomorrow\n- Second task",))
    first.start()
    assert entered.wait(5)
    second.start()
    try:
        assert not second_record.wait(0.2), "a second save reached promise writes before the first save finished"
    finally:
        release.set()
        first.join(5)
        second.join(5)
    assert not first.is_alive() and not second.is_alive() and not errors
    assert pm_learn.pm_sent_text("2026-10-30") == "Tomorrow\n- Second task"
    assert [r["title"] for r in promises.open_list()] == ["Second task"]


def test_restored_bullet_reopens_dropped_but_not_completed_promise():
    original = "Tomorrow\n- First task"
    pm_learn.save_sent("2026-10-30", original)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Replacement task")
    pm_learn.save_sent("2026-10-30", original)
    rows = promises.open_list()
    assert [r["title"] for r in rows] == ["First task"]
    promises.close(rows[0]["name"])
    pm_learn.save_sent("2026-10-30", original)
    assert not promises.open_list()


def test_legacy_sent_update_is_visible_then_reconciled_at_day_start():
    from careerbrew import store
    from careerbrew.workday import start

    store.write_text(pm_learn.sent_path("2026-10-30"), TEXT)
    before = projects.path().read_bytes()
    got = plan.build(date(2026, 11, 2))["pm_promises"]
    assert not got["missing"] and len(got["items"]) == 3
    assert got["needs_recording"] and not promises.open_list()
    assert projects.path().read_bytes() == before, "reading the plan must not write promise state"
    config.set_value("work_checkouts", "")
    start.run([])
    assert len(promises.open_list()) == 3
    assert not plan.build(date(2026, 11, 2))["pm_promises"]["needs_recording"]


def test_owner_closure_of_a_removed_bullet_remains_authoritative():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- First task")
    name = promises.open_list()[0]["name"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Replacement task")
    promises.close(name, "dropped")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- First task")
    assert not promises.open_list()
    assert plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]["status"] == "dropped"


def test_promise_sources_do_not_steal_project_matching_on_a_sent_edit():
    projects.add("invoices")
    projects.add_source("invoices", "url", "https://tracker.example.test/export/41")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver export https://tracker.example.test/export/41")
    assert promises.open_list()[0]["parent"] == "invoices"
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver export and share it https://tracker.example.test/export/41")
    assert promises.open_list()[0]["parent"] == "invoices"


def test_case_only_duplicate_bullets_get_one_coverage_and_report_row():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver billing export\n- deliver BILLING export\n")
    assert len(promises.open_list()) == 1
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 1
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert text.count("Yesterday's promise:") == 1


def test_parent_match_keeps_overlapping_project_globs_unmatched():
    projects.add("first")
    projects.add("second")
    config.set_value("project_priority", "first=*export*, second=*export*")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver export")
    assert promises.open_list()[0]["parent"] == ""


def test_parent_match_filters_archived_projects_before_matching():
    projects.add("first")
    projects.add("second")
    projects.archive("first")
    config.set_value("project_priority", "first=*export*, second=*export*")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver export")
    assert promises.open_list()[0]["parent"] == "second"


def test_parent_match_reads_repository_globs_from_a_pr_link():
    config.set_value("project_priority", "billing=sample/export")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review https://github.com/sample/export/pull/12")
    assert promises.open_list()[0]["parent"] == "billing"


def test_wrapped_bullet_keeps_its_continuation_and_later_promises():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Deliver export\n  and send the result\n- Review rollout")
    assert {row["title"] for row in promises.open_list()} == {"Deliver export and send the result", "Review rollout"}


@pytest.mark.parametrize(
    ("sources", "bullet", "parent"),
    [
        pytest.param([("ledger", "repo", "acme/billing")], "Review https://github.com/acme/payroll/pull/12", "", id="another-repo-under-owner"),
        pytest.param([("ledger", "url", "https://tracker.example.test/work/41")], "Review https://tracker.example.test/work/42", "", id="another-url-on-host"),
        pytest.param([("ledger", "url", "https://tracker.example.test/work/41?view=full#summary")], "Review https://tracker.example.test/work/41?view=other#summary", "", id="different-url-query"),
        pytest.param([("ledger", "url", "https://tracker.example.test/work/41?view=full#summary")], "Review https://tracker.example.test/work/41?view=full#other", "", id="different-url-fragment"),
        pytest.param([("ledger", "tracker", "PROJ-41")], "Review https://tracker.example.test/browse/PROJ-42", "", id="another-ticket-under-prefix"),
        pytest.param([("ledger", "repo", "acme/billing")], "Review https://unrelated.example.test/ledger-tools/overview", "", id="name-in-url-is-not-mention"),
        pytest.param([("alpha", "url", "https://github.com/acme/export/pull/12"), ("beta", "repo", "acme/export")], "Review https://github.com/acme/export/pull/12", "", id="mixed-exact-source-tie"),
        pytest.param([("ledger", "url", "https://example.slack.com/archives/C0123/p111")], "Review https://example.slack.com/archives/C0123/p222", "", id="thread-reference-stays-exact"),
        pytest.param([("ledger", "repo", "acme/billing")], "Review https://github.com/acme/billing/pull/12", "ledger", id="exact-repository"),
        pytest.param([("ledger", "url", "https://tracker.example.test/work/41?view=full#summary")], "Review https://TRACKER.example.test/work/41?view=full#summary", "ledger", id="exact-url-host-case"),
        pytest.param([("ledger", "tracker", "PROJ-41")], "Review https://tracker.example.test/browse/PROJ-41", "ledger", id="exact-ticket"),
        pytest.param([("ledger", "chat", "C0123")], "Review https://example.slack.com/archives/C0123/p111", "ledger", id="declared-channel"),
        pytest.param([("ledger", "doc", "ExampleDoc42")], "Review https://docs.google.com/document/d/ExampleDoc42/edit", "ledger", id="exact-document"),
        pytest.param([("ledger", "repo", "acme/billing")], "Review ledger output https://unrelated.example.test/notes", "ledger", id="explicit-name-outside-url"),
    ],
)
def test_parent_scope_uses_exact_declared_sources(sources, bullet, parent):
    config.set_value("project_priority", "")
    for name, kind, ref in sources:
        if name not in projects.records():
            projects.add(name)
        projects.add_source(name, kind, ref)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + bullet)
    assert promises.open_list()[0]["parent"] == parent


@pytest.mark.parametrize(
    ("source", "link", "parent"),
    [
        ("https://tracker.example.test:8443/work/41", "https://tracker.example.test:8444/work/41", ""),
        ("https://[FE80::1%25en0]:8443/work/41", "https://[fe80::1%25en0]:8443/work/41", "ledger"),
        ("https://[fe80::1%25en0]:8443/work/41", "https://[fe80::1%25En0]:8443/work/41", ""),
        ("https://Reader@tracker.example.test/work/41", "https://reader@tracker.example.test/work/41", ""),
        ("https://straße.example.test/work/41", "https://strasse.example.test/work/41", ""),
    ],
)
def test_url_authority_identity_controls(source, link, parent):
    config.set_value("project_priority", "")
    projects.add("ledger")
    projects.add_source("ledger", "url", source)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review " + link)
    assert promises.open_list()[0]["parent"] == parent


@pytest.mark.parametrize(
    ("source", "target", "parent"),
    [
        ("https://tracker.example.test/work/41", "HTTPS://unrelated.example.test/ledger-tools/overview", ""),
        ("https://tracker.example.test/work/41", "HTTPS://TRACKER.example.test/work/41", "ledger"),
        ("https://tracker.example.test/work/41", "https://tracker.example.test/work/41.", ""),
        ("https://tracker.example.test/work/41", "https://tracker.example.test/work/41)", ""),
        ("https://tracker.example.test/work/41?q=done", "https://tracker.example.test/work/41?q=done.", ""),
        ("https://tracker.example.test/work/41#done", "https://tracker.example.test/work/41#done)", ""),
    ],
)
def test_review_url_identity_preserves_explicit_target(source, target, parent):
    config.set_value("project_priority", "")
    projects.add("ledger")
    projects.add_source("ledger", "url", source)
    pm_learn.save_sent("2026-10-30", f"Tomorrow\n- Review <{target}>")
    row = promises.open_list()[0]
    assert row["parent"] == parent
    assert projects.records()[row["name"]]["sources"][0]["ref"] == target


@pytest.mark.parametrize(
    ("kind", "source", "target", "parent"),
    [
        ("url", "https://tracker.example.test/work/41", "https://tracker.example.test/work/41?", ""),
        ("url", "https://tracker.example.test/work/41", "https://tracker.example.test/work/41#", ""),
        ("url", "https://tracker.example.test/work/41?", "https://tracker.example.test/work/41?", "ledger"),
        ("url", "https://tracker.example.test/work/41#", "https://tracker.example.test/work/41#", "ledger"),
        ("repo", "acme/billing", "https://notgithub.com/acme/billing/pull/12", ""),
        ("repo", "acme/billing", "https://github.attacker.test/acme/billing/pull/12", ""),
        ("repo", "acme/billing", "https://github.com/acme/billing/pull/12", "ledger"),
    ],
    ids=["extra-query-marker", "extra-fragment-marker", "declared-query-marker", "declared-fragment-marker", "github-suffix-lookalike", "github-prefix-lookalike", "trusted-github"],
)
def test_complete_url_scope_has_exact_declared_identity(kind, source, target, parent):
    config.set_value("project_priority", "")
    projects.add("ledger")
    projects.add_source("ledger", kind, source)
    pm_learn.save_sent("2026-10-30", f"Tomorrow\n- Review <{target}>")
    row = promises.open_list()[0]
    assert row["parent"] == parent
    assert projects.records()[row["name"]]["sources"][0]["ref"] == target


@pytest.mark.parametrize("host,parent", [("notgithub.com", ""), ("github.attacker.test", ""), ("github.com", "ledger")])
def test_repo_pattern_authority_uses_the_declared_github_origin(host, parent):
    projects.add("ledger")
    config.set_value("project_priority", "ledger=acme/billing")
    pm_learn.save_sent("2026-10-30", f"Tomorrow\n- Review <https://{host}/acme/billing/pull/12>")
    assert promises.open_list()[0]["parent"] == parent


@pytest.mark.parametrize("mode", ["source", "ranked"])
@pytest.mark.parametrize(
    "apis,target,parent",
    [
        (["https://code.example.test:8443/api/v3"], "https://code.example.test:8443/acme/billing/pull/12", "ledger"),
        (["https://code.example.test:8443/api/v3"], "https://github.com/acme/billing/pull/12", ""),
        (["https://code.example.test:8443/api/v3"], "https://code.attacker.test:8443/acme/billing/pull/12", ""),
        (["https://code.example.test:8443/api/v3"], "https://code.example.test:8444/acme/billing/pull/12", ""),
        (["https://api.github.com"], "https://github.com/acme/billing/pull/12", "ledger"),
        ([], "https://github.com/acme/billing/pull/12", "ledger"),
        (["https://api.github.com", "https://code.example.test:8443/api/v3"], "https://github.com/acme/billing/pull/12", ""),
    ],
    ids=["enterprise", "public-collision", "unconfigured-host", "wrong-port", "public-api", "default-public", "ambiguous-origins"],
)
def test_ghe_origin_scope_uses_configured_provider_authority(monkeypatch, mode, apis, target, parent):
    from careerbrew.work import evidence

    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "github", "api": api} for api in apis])
    projects.add("ledger")
    config.set_value("project_priority", "ledger=acme/billing" if mode == "ranked" else "")
    if mode == "source":
        projects.add_source("ledger", "repo", "acme/billing")
    pm_learn.save_sent("2026-10-30", f"Tomorrow\n- Review <{target}>")
    assert promises.open_list()[0]["parent"] == parent


@pytest.mark.parametrize(
    "first,second",
    [
        ("https://tracker.example.test/A", "https://tracker.example.test/a"),
        ("https://tracker.example.test/item?q=A", "https://tracker.example.test/item?q=a"),
        ("https://tracker.example.test/item#A", "https://tracker.example.test/item#a"),
    ],
    ids=["path", "query", "fragment"],
)
def test_url_bullet_identity_scope_keeps_distinct_targets(first, second):
    titles = [f"Review <{first}>", f"Review <{second}>"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n" + "\n".join("- " + title for title in titles))
    assert {row["title"] for row in promises.open_list()} == set(titles)
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 2 and {row["title"] for row in items} == set(titles)
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert all(title in text for title in titles) and text.count("Yesterday's promise:") == 2


def test_url_bullet_identity_scope_changed_completed_target_is_open():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review <https://tracker.example.test/A>")
    old = promises.open_list()[0]["name"]
    promises.close(old)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review <https://tracker.example.test/a>")
    rows = promises.open_list()
    assert len(rows) == 1 and rows[0]["name"] != old
    assert rows[0]["title"] == "Review <https://tracker.example.test/a>" and not rows[0]["closed"]


def test_url_bullet_identity_scope_keeps_prose_scheme_host_case_deduplication():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review <https://TRACKER.example.test/A>\n- review <HTTPS://tracker.example.test/A>")
    assert len(promises.open_list()) == 1
    assert len(plan.build(date(2026, 11, 2))["pm_promises"]["items"]) == 1


def _legacy_pm_promise(title):
    name = promises.name_for("pm-update:2026-10-30:" + title.casefold())
    projects.sync_pm_promises(
        "2026-10-30",
        [
            {
                "name": name,
                "title": title,
                "parent": "",
                "link": "",
                "promise": {"source": "pm-update", "pm_day": "2026-10-30", "event": "pm-update:2026-10-30:" + name, "due": "2026-11-02", "weight": 100, "to": "", "made_at": "2026-10-30T00:00:00", "steps": {}},
            }
        ],
    )
    return name


def test_url_bullet_identity_scope_legacy_same_target_keeps_owner_closure():
    name = _legacy_pm_promise("Review <https://TRACKER.example.test/A>")
    promises.close(name)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- review <HTTPS://tracker.example.test/A>")
    row = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert row["promise"] == name and row["status"] == "done"
    assert row["title"] == "Review <https://TRACKER.example.test/A>"


def test_url_bullet_identity_scope_legacy_changed_target_does_not_inherit_closure():
    name = _legacy_pm_promise("Review <https://tracker.example.test/A>")
    promises.close(name)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Review <https://tracker.example.test/a>")
    row = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert row["promise"] != name and row["status"] == "still open"
    assert row["title"] == "Review <https://tracker.example.test/a>"


def test_url_bullet_identity_scope_no_url_legacy_keeps_owner_closure():
    name = _legacy_pm_promise("Deliver billing export")
    promises.close(name)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- deliver BILLING export")
    row = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert row["promise"] == name and row["status"] == "done" and row["title"] == "Deliver billing export"


def test_url_bullet_identity_scope_legacy_source_removal_restores_original_id():
    title = "Review <https://tracker.example.test/A>"
    name = _legacy_pm_promise(title)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- Replacement")
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    row = promises.open_list()[0]
    assert row["name"] == name and row["title"] == title and not row["closed"]


def test_malformed_url_identity_scope_saves_exact_unmatched_target():
    title = "Review <https://[invalid>"
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    row = promises.open_list()[0]
    assert row["title"] == title and row["parent"] == ""
    assert projects.records()[row["name"]]["sources"][0]["ref"] == "https://[invalid"


def test_malformed_url_identity_scope_legacy_preview_stays_read_only():
    from careerbrew import store

    store.write_text(pm_learn.sent_path("2026-10-30"), "Tomorrow\n- Review <https://[invalid>")
    before = projects.path().read_bytes()
    got = plan.build(date(2026, 11, 2))["pm_promises"]
    assert got["needs_recording"] and len(got["items"]) == 1
    assert got["items"][0]["title"] == "Review <https://[invalid>"
    assert projects.path().read_bytes() == before and not promises.open_list()


def test_malformed_url_identity_scope_keeps_invalid_and_valid_targets():
    titles = ["Review <https://[invalid>", "Review <https://tracker.example.test/A>"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n" + "\n".join("- " + title for title in titles))
    assert {row["title"] for row in promises.open_list()} == set(titles)
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 2 and len({row["promise"] for row in items}) == 2
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert all(title in text for title in titles) and text.count("Yesterday's promise:") == 2


def test_malformed_url_identity_scope_keeps_raw_target_case_distinct():
    titles = ["Review <https://[Invalid>", "Review <https://[invalid>"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n" + "\n".join("- " + title for title in titles))
    assert {row["title"] for row in promises.open_list()} == set(titles)
    assert len(plan.build(date(2026, 11, 2))["pm_promises"]["items"]) == 2


def test_malformed_url_identity_scope_repeated_target_keeps_owner_closure():
    text = "Tomorrow\n- Review <https://[invalid>"
    pm_learn.save_sent("2026-10-30", text)
    name = promises.open_list()[0]["name"]
    promises.close(name)
    pm_learn.save_sent("2026-10-30", text)
    row = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert row["promise"] == name and row["status"] == "done"
    assert row["title"] == "Review <https://[invalid>" and not promises.open_list()


NAMESPACE_URL = "https://github.com/acme/export"
NAMESPACE_PROSE = 'url-v2:["",["https","github.com","/acme/export","","",false,false],""]'


def test_namespace_collision_scope_keeps_url_and_serialized_key_prose():
    titles = [NAMESPACE_URL, NAMESPACE_PROSE]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n" + "\n".join("- " + title for title in titles))
    assert {row["title"] for row in promises.open_list()} == set(titles)
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 2 and len({row["promise"] for row in items}) == 2
    assert {row["title"] for row in items} == set(titles)
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert all(title in text for title in titles) and text.count("Yesterday's promise:") == 2


def test_namespace_collision_scope_replacement_cannot_inherit_url_closure():
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_URL)
    old = promises.open_list()[0]["name"]
    promises.close(old)
    history = projects.records()[old]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_PROSE)
    rows = promises.open_list()
    assert len(rows) == 1 and rows[0]["name"] != old and rows[0]["title"] == NAMESPACE_PROSE
    assert projects.records()[old] == history
    item = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert item["promise"] == rows[0]["name"] and item["title"] == NAMESPACE_PROSE and item["status"] == "still open"
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert NAMESPACE_PROSE in text and "still open" in text


def _namespace_url_v2_promise():
    name = promises.name_for("pm-update:2026-10-30:" + NAMESPACE_PROSE)
    projects.sync_pm_promises(
        "2026-10-30",
        [
            {
                "name": name,
                "title": NAMESPACE_URL,
                "parent": "",
                "link": NAMESPACE_URL,
                "promise": {"source": "pm-update", "pm_day": "2026-10-30", "event": "pm-update:2026-10-30:" + name, "due": "2026-11-02", "weight": 100, "to": "", "made_at": "2026-10-30T00:00:00", "steps": {}},
            }
        ],
    )
    return name


def test_namespace_legacy_control_preserves_url_v2_id_and_closure():
    old = _namespace_url_v2_promise()
    promises.close(old)
    history = projects.records()[old]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- HTTPS://GITHUB.COM/acme/export")
    item = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert item["promise"] == old and item["status"] == "done"
    assert projects.records()[old] == history and not promises.open_list()


def test_namespace_legacy_control_keeps_occupied_prose_fallback_stable():
    old = _namespace_url_v2_promise()
    promises.close(old)
    history = projects.records()[old]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_URL + "\n- " + NAMESPACE_PROSE)
    rows = promises.open_list()
    assert len(rows) == 1 and rows[0]["name"] != old and rows[0]["title"] == NAMESPACE_PROSE
    fallback = rows[0]["name"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_PROSE + "\n- " + NAMESPACE_URL)
    assert [row["name"] for row in promises.open_list()] == [fallback]
    assert projects.records()[old] == history
    projects.remove(old)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_PROSE)
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_PROSE)
    assert [row["name"] for row in promises.open_list()] == [fallback]
    assert plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]["promise"] == fallback


def test_namespace_legacy_control_existing_prose_cannot_steal_url_identity():
    old = _legacy_pm_promise(NAMESPACE_PROSE)
    promises.close(old)
    history = projects.records()[old]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + NAMESPACE_PROSE + "\n- " + NAMESPACE_URL)
    rows = promises.open_list()
    assert len(rows) == 1 and rows[0]["name"] != old and rows[0]["title"] == NAMESPACE_URL
    assert projects.records()[old] == history
    items = plan.build(date(2026, 11, 2))["pm_promises"]["items"]
    assert len(items) == 2 and {row["promise"] for row in items} == {old, rows[0]["name"]}


LONG_PROSE = "Deliver the synthetic promise " + "details " * 35 + "ending"
LONG_LATE_URL = "Review " + "synthetic details " * 16 + "https://tracker.example.test/item/A"
LONG_PATH = "path" * 55 + "A"
LONG_URL = "https://tracker.example.test/" + LONG_PATH


def _assert_long_promise_view(title, name, status, monkeypatch, capsys):
    import re

    record = projects.records()[name]
    assert record["title"] == title[:200].strip()
    item = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
    assert item["promise"] == name and item["title"] == record["title"] and item["status"] == status
    text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
    assert record["title"] in text and status in text
    proof = record["promise"].get("identity_digest")
    if proof is not None:
        assert re.fullmatch(r"[0-9a-f]{24}", proof) and title not in proof
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    public = capsys.readouterr().out
    assert record["title"] not in public and "identity_digest" not in public


@pytest.mark.parametrize("title", [LONG_PROSE, LONG_LATE_URL], ids=["prose", "url-after-display-limit"])
def test_long_identity_scope_repeat_keeps_one_stable_record(title, monkeypatch, capsys):
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    name = promises.open_list()[0]["name"]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    assert [row["name"] for row in promises.open_list()] == [name]
    assert projects.records()[name]["promise"]["identity_digest"]
    _assert_long_promise_view(title, name, "still open", monkeypatch, capsys)


@pytest.mark.parametrize("title", [LONG_PROSE, LONG_LATE_URL], ids=["prose", "url-after-display-limit"])
def test_long_identity_scope_closed_record_keeps_owner_closure(title, monkeypatch, capsys):
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    name = promises.open_list()[0]["name"]
    promises.close(name)
    history = projects.records()[name]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + title)
    assert not promises.open_list() and projects.records()[name] == history
    assert history["promise"]["identity_digest"]
    _assert_long_promise_view(title, name, "done", monkeypatch, capsys)


def test_long_identity_scope_legacy_url_v2_keeps_proven_closure(monkeypatch, capsys):
    key = 'url-v2:["",["https","tracker.example.test","/' + LONG_PATH + '","","",false,false],""]'
    name = promises.name_for("pm-update:2026-10-30:" + key)
    projects.sync_pm_promises(
        "2026-10-30",
        [
            {
                "name": name,
                "title": LONG_URL,
                "parent": "",
                "link": LONG_URL,
                "promise": {"source": "pm-update", "pm_day": "2026-10-30", "event": "pm-update:2026-10-30:" + name, "due": "2026-11-02", "weight": 100, "to": "", "made_at": "2026-10-30T00:00:00", "steps": {}},
            }
        ],
    )
    promises.close(name)
    history = projects.records()[name]
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + LONG_URL)
    assert not promises.open_list() and projects.records()[name] == history
    _assert_long_promise_view(LONG_URL, name, "done", monkeypatch, capsys)


def test_long_identity_scope_changed_url_tail_is_distinct_and_open(monkeypatch, capsys):
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + LONG_URL)
    old = promises.open_list()[0]["name"]
    promises.close(old)
    history = projects.records()[old]
    changed = LONG_URL[:-1] + "a"
    pm_learn.save_sent("2026-10-30", "Tomorrow\n- " + changed)
    rows = promises.open_list()
    assert len(rows) == 1 and rows[0]["name"] != old
    name = rows[0]["name"]
    assert projects.records()[old] == history
    assert projects.records()[name]["promise"]["identity_digest"] != history["promise"]["identity_digest"]
    _assert_long_promise_view(changed, name, "still open", monkeypatch, capsys)


@pytest.mark.parametrize(
    "heading",
    [
        pytest.param("Tomorrow (Mon)", id="P01"),
        pytest.param("## Tomorrow - Monday:", id="P02"),
        pytest.param("**Next week**", id="P03"),
    ],
)
def test_issue389_heading_primary(heading, tmp_path):
    from careerbrew.day import pm_promises

    text = f"Today\n- Shipped an earlier sample\n\n{heading}\n- Ship the sample\n\nRisks\n- Do not track this"
    pm_learn.save_sent("2026-10-30", text)
    pm_learn.save_sent("2026-10-30", text)

    opened = [(row["name"], row["title"], row["due"]) for row in promises.open_list()]
    visible = [(row["promise"], row["title"], row["due"], row["recorded"]) for row in pm_promises.view(date(2026, 11, 2))["items"]]
    (tmp_path / "issue389-heading.json").write_text(json.dumps({"open": opened, "visible": visible}), encoding="utf-8")

    assert len(opened) == len(visible) == 1
    name, title, due = opened[0]
    assert (title, due) == ("Ship the sample", "2026-11-02")
    assert visible == [(name, title, due, True)]


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param("Today\n- Earlier\n*Tomorrow*\n- Keep\nRisks\n- Ignore", ["Keep"], id="C01"),
        pytest.param("Tomorrow roadmap\n- Ignore\nTomorrow (Monsoon)\n- Ignore\nNext week preview\n- Ignore", [], id="C02"),
        pytest.param("Tomorrow\n- Keep\nToday\n- Ignore\nTomorrow\n- Also ignore", ["Keep"], id="C03"),
        pytest.param("Tomorrow\n- Ship\n  the sample\n\n1. Check\n2. Check", ["Ship the sample", "Check"], id="C04"),
        pytest.param("Today\n* Tomorrow (Mon)\n- Ignore\n* Next week\n- Also ignore", [], id="C05"),
        pytest.param("Tomorrow\n* Tomorrow (Mon)\n- Keep", ["Tomorrow (Mon)", "Keep"], id="C06"),
    ],
)
def test_issue389_heading_control(text, expected, tmp_path):
    from careerbrew.day import pm_promises

    actual = pm_promises.bullets(text)
    (tmp_path / "issue389-heading.json").write_text(json.dumps(actual), encoding="utf-8")
    assert actual == expected


@pytest.mark.parametrize(
    "text",
    [
        "Tomorrow\n- First\nNext week\n- Second",
        "Next week\n- First\nTomorrow (Mon)\n- Second",
        "Next week\n- First\nTomorrow\n- Second",
        "Tomorrow (Mon)\n- First\nTomorrow\n- Second",
    ],
)
def test_issue389_adjacent_future_section_stops(text):
    from careerbrew.day import pm_promises

    assert pm_promises.bullets(text) == ["First"]


def test_issue389_repeated_plain_tomorrow_keeps_legacy_behavior():
    from careerbrew.day import pm_promises

    assert pm_promises.bullets("Tomorrow\n- First\nTomorrow\n- Second") == ["First", "Second"]
