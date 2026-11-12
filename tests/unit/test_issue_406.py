"""Declared direct coverage and visible source health (registered 406-01–18)."""

import json
import os
import shutil
from datetime import date, datetime
from urllib.parse import urlsplit

import pytest

from careerbrew import cli, config, server, store
from careerbrew.day import log, results
from careerbrew.day import watch as day_watch
from careerbrew.work import feed, live, standup
from careerbrew.workday import watch


@pytest.fixture
def clock(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:00:00+00:00")
    cli.main(["init"])
    return home


@pytest.mark.parametrize("as_json", [False, True], ids=["text", "json"])
def test_406_01_12_no_source_pass_has_no_false_success(clock, capsys, as_json):
    capsys.readouterr()
    code = cli.main(["day", "loop", "pass", *(["--json"] if as_json else [])])
    output = capsys.readouterr().out
    assert code == 1, "an active pass without declared acquisition coverage must fail"
    if as_json:
        assert json.loads(output)["status"] == "failed"


@pytest.mark.parametrize("view", [server.today, server.standup_view], ids=["today", "standup"])
def test_406_13_missing_source_warning_is_visible(clock, view):
    health = view().get("feed_health")
    assert health is not None, "the view must expose its missing source coverage notice"
    assert health["status"] == "missing"
    assert health["commands"], "the notice must name an existing setup command"


def test_406_08_eligible_direct_action_reaches_today_and_done(clock):
    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
    feed.ingest([{"id": "pagerduty:resolved-1", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Resolved synthetic outage", "actor": "me", "status": "resolved"}])
    assert len(feed.evidence_items()) == 1, "the fixture is an eligible own action before completion"
    feed.mark_cycle()
    assert [row["id"] for row in results.load()] == ["pagerduty:resolved-1"]
    assert "Resolved synthetic outage" in [title for group in log.build()["done"].values() for title in group]


def health():
    return server.view(urlsplit("/api/feed/health"))


def declare():
    config.set_value("feed_tool_sources", "mcp__tickets__*=tickets,mcp__chat__*=chat")


def quiet(*sources):
    for source in sources:
        feed.ingest([], sources=[source])


def test_406_01_unknown_batch_cannot_certify_setup(clock):
    quiet("unknown")
    feed.mark_cycle(["unknown"])
    assert health()["status"] == "missing"
    assert health()["mode"] == "none"


def test_406_02_stopped_adapters_need_refresh(clock, tmp_path):
    live.save_sources(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(tmp_path / "inbox")}]}))
    got = health()
    assert got["status"] == "stopped" and not got["running"]
    assert got["commands"] == ["careerbrew work live start"]


def test_406_03_complete_direct_coverage_does_not_start_adapters(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    got = health()
    assert got["status"] == "healthy" and got["mode"] == "direct"
    assert (got["required"], got["healthy"]) == (2, 2)
    assert got["running"] is False


@pytest.mark.parametrize("requested,stored", [(["tickets"], ["tickets"]), (["unknown"], ["unknown"])])
def test_406_04_explicit_batch_cannot_shrink_declared_coverage(clock, requested, stored):
    declare()
    quiet(*stored)
    feed.mark_cycle(requested)
    assert health()["status"] != "healthy"


def test_406_05_failed_latest_batch_overrides_earlier_success(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert health()["status"] == "healthy"
    quiet("tickets")
    feed.mark_cycle()
    assert health()["status"] == "error"


def test_406_06_11_health_read_does_not_renew_expiry_or_write(clock, monkeypatch):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    before = {p.relative_to(clock): p.read_bytes() for p in clock.rglob("*") if p.is_file()}
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:15:00+00:00")
    assert health()["status"] == "healthy"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:15:01+00:00")
    assert health()["status"] == "stale"
    assert health()["status"] == "stale"
    after = {p.relative_to(clock): p.read_bytes() for p in clock.rglob("*") if p.is_file()}
    assert after == before


def test_406_07_quiet_duplicate_completion_is_one_explicit_cycle(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    first = store.read_json(clock / "work/feed/direct-cycle.json")
    health()
    assert store.read_json(clock / "work/feed/direct-cycle.json") == first
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert store.read_json(clock / "work/feed/direct-cycle.json")["cycles"] == 2
    assert feed.load() == []


@pytest.mark.parametrize("extra", [{"personal": True}, {"actor": "other"}, {"kind": "dm", "source": "chat"}])
def test_406_09_ineligible_direct_items_do_not_become_results(clock, extra):
    declare()
    feed.ingest([{"id": "event-1", "source": "chat", "kind": "dm", "at": "2026-11-05T11:55:00Z", "title": "Ordinary conversation", "actor": "me", **extra}])
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert results.load() == []


def test_406_10_direct_versions_keep_the_event_local_day(clock, monkeypatch):
    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
    event = {"id": "stable-page", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Original", "actor": "me", "status": "resolved"}
    feed.ingest([event])
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-06T12:00:00+00:00")
    feed.ingest([{**event, "title": "Corrected"}])
    feed.mark_cycle()
    assert [(r["id"], r["title"]) for r in results.load(date(2026, 11, 5))] == [("stable-page", "Corrected")]
    assert results.load() == []


def test_406_11_direct_receipt_changes_view_version(clock):
    declare()
    before = server.changes_view()["version"]
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert server.changes_view()["version"] != before


def test_406_14_15_fresh_activity_does_not_review_old_row_facts(clock, monkeypatch):
    standup.save_rows('{"rows":[{"title":"Stable definition","status":"In progress","owner":"me"}]}')
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T13:01:00+00:00")
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    got = health()
    assert got["status"] == "healthy"
    assert got["row_review"]["status"] == "due"
    assert standup.load_rows()["rows"][0]["title"] == "Stable definition"
    assert health()["row_review"] == got["row_review"]


@pytest.mark.parametrize("saved_at", ["", "bad", "2026-11-05T13:00:00+00:00"])
def test_406_16_invalid_or_future_row_review_is_unknown(clock, saved_at):
    store.write_json(standup.rows_path(), {"rows": [{"title": "Stable definition"}], "saved_at": saved_at})
    assert health()["row_review"]["status"] == "unknown"


def test_406_05_malformed_latest_input_cannot_reuse_good_pending_coverage(clock):
    declare()
    quiet("tickets", "chat")
    with pytest.raises(feed.FeedError):
        feed.add_lines("malformed", source="chat")
    completed = feed.mark_cycle()
    assert completed["not_stored"], "malformed current input must fail the pending batch"
    assert health()["status"] == "error"


def test_406_06_future_and_reversed_acquisition_is_unknown(clock, monkeypatch):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T11:59:59+00:00")
    assert health()["status"] == "unknown"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:01:00+00:00")
    quiet("tickets", "chat")
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:00:30+00:00")
    assert health()["status"] == "unknown"


def test_406_04_changed_config_invalidates_direct_receipt(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    config.set_value("feed_tool_sources", "mcp__other__*=tickets,mcp__chat__*=chat")
    assert health()["status"] != "healthy"


def test_406_04_live_required_source_is_part_of_direct_coverage(clock, tmp_path):
    declare()
    live.save_sources(json.dumps({"sources": [{"name": "calendar", "kind": "calendar", "required": True, "inbox": str(tmp_path / "inbox")}]}))
    quiet("tickets", "chat")
    got = feed.mark_cycle(["tickets"])
    assert got["not_stored"] == ["calendar"]
    assert health()["status"] != "healthy"


def test_406_11_health_does_not_call_intake_or_publish(clock, monkeypatch):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()

    def forbidden(*args, **kwargs):
        raise AssertionError("read-only health attempted a write or intake")

    monkeypatch.setattr(store, "write_json", forbidden)
    monkeypatch.setattr(store, "write_jsonl", forbidden)
    monkeypatch.setattr(feed, "ingest", forbidden)
    monkeypatch.setattr(feed, "pull", forbidden)
    monkeypatch.setattr(live, "cycle", forbidden)
    monkeypatch.setattr(standup, "run", forbidden)
    assert health()["status"] == "healthy"


def test_406_12_direct_health_does_not_record_a_running_adapter(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert watch.run_pass()["status"] == "ok"
    assert not live._state()["running"]
    assert all(row["kind"] != "live-feed" for row in watch.status()["running"])


def test_406_16_today_provenance_is_exact_not_an_id_prefix(clock):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    results.append([{"id": "chat:ambiguous", "at": "2026-11-05T11:55:00Z", "title": "Old unbound record", "kind": "note", "source": "evidence"}])
    row = next(r for r in server.today()["results"] if r["id"] == "chat:ambiguous")
    assert row["source_health"]["health"] == "unknown"


def test_406_13_invalid_configuration_has_finite_safe_reason(clock):
    store.write_yaml(clock / "work/live-sources.yaml", {"sources": [{"name": "invalid-canary", "inbox": "/synthetic/private/input"}]})
    got = health()
    assert got["status"] == "error" and got["reason"] == "invalid-config"
    assert "/synthetic/private/input" not in json.dumps(got)


def test_406_13_first_current_adapter_error_is_not_reported_as_unknown(clock, tmp_path):
    inbox = tmp_path / "reply.json"
    inbox.write_text("malformed")
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(inbox)}]}))
    assert live.cycle()["status"] == "failed"
    assert health()["status"] == "error"


def test_406_06_reversed_completion_preserves_prior_check_highwater(clock, monkeypatch):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T11:59:00+00:00")
    quiet("tickets", "chat")
    feed.mark_cycle()
    assert health()["status"] == "unknown"
    assert store.read_json(clock / "work/feed/direct-cycle.json")["checked_at"] == "2026-11-05T12:00:00+00:00"


def test_406_04_an_explicit_extra_required_gap_cannot_be_ignored(clock):
    declare()
    quiet("tickets", "chat")
    assert feed.mark_cycle(["extra"])["not_stored"] == ["extra"]
    assert health()["status"] == "error"


def test_406_10_direct_intake_supplies_canonical_provenance_to_existing_result(clock):
    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
    event = {"id": "page-1", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Resolved", "actor": "me", "status": "resolved"}
    results.append([{"id": "page-1", "at": event["at"], "kind": "pagerduty", "title": "Resolved", "source": "evidence", "origin": "user"}])
    feed.ingest([event])
    feed.mark_cycle()
    assert len(results.load()) == 1
    assert results.load()[0]["feed_source"] == "pagerduty"


def test_406_16_invalid_check_metadata_is_not_copied_to_health_output(clock):
    store.write_json(standup.rows_path(), {"rows": [{"title": "Stable definition"}], "saved_at": "PRIVATE-TIME-CANARY"})
    assert "PRIVATE-TIME-CANARY" not in json.dumps(health())


def test_406_13_invalid_configuration_preserves_adapter_running_truth(clock):
    store.write_json(live.state_path(), {"running": True})
    store.write_yaml(clock / "work/live-sources.yaml", {"sources": [{"name": "invalid"}]})
    assert health()["running"] is True


def test_406_07_quiet_batch_reuses_standup_due_policy_without_republishing(clock, monkeypatch):
    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
    standup.save_rows('{"rows":[{"title":"Synthetic outage","aliases":["outage"]}]}')
    feed.ingest([{"id": "page-1", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Resolved synthetic outage", "actor": "me", "status": "resolved"}])
    feed.mark_cycle()
    original = standup.run
    calls = []

    def observe(*args, **kwargs):
        calls.append(1)
        return original(*args, **kwargs)

    monkeypatch.setattr(standup, "run", observe)
    quiet("pagerduty")
    feed.mark_cycle()
    assert calls == [], "a fresh quiet completion is not new matching activity"


def test_406_11_health_does_not_create_missing_source_directories(clock):
    shutil.rmtree(clock / "work")
    assert health()["status"] == "missing"
    assert not (clock / "work").exists()


def test_406_04_a_tool_mapping_makes_an_optional_adapter_identity_required(clock, tmp_path):
    config.set_value("feed_tool_sources", "mcp__tickets__*=tickets")
    inbox = tmp_path / "reply.json"
    inbox.write_text("malformed")
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": False, "inbox": str(inbox)}]}))
    live.cycle()
    assert health()["status"] == "error"
    assert health()["sources"][0]["required"] is True
    assert watch.run_pass()["status"] == "failed"


@pytest.mark.parametrize("case,want", [("later-error", "error"), ("later-direct", "healthy"), ("equal-time", "error")])
def test_406_05_cross_path_precedence(clock, tmp_path, monkeypatch, case, want):
    inbox = tmp_path / "reply.json"
    inbox.write_text('{"tickets":[]}')
    at = datetime.fromisoformat("2026-11-05T12:00:00+00:00").timestamp()
    os.utime(inbox, (at, at))
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(inbox)}]}))
    live.cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:01:00+00:00")
    quiet("tickets")
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:02:00+00:00")
    inbox.write_text("malformed")
    live.cycle()
    if case != "later-error":
        monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:03:00+00:00" if case == "later-direct" else "2026-11-05T12:02:00+00:00")
        quiet("tickets")
        feed.mark_cycle()
    got = health()
    assert got["status"] == want
    assert got["running"] is True


@pytest.mark.parametrize("as_json", [False, True], ids=["text", "json"])
def test_406_06_unknown_clock_completion_fails_without_changing_physical_counts(clock, monkeypatch, capsys, as_json):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T11:59:00+00:00")
    quiet("tickets", "chat")
    capsys.readouterr()
    code = cli.main(["work", "feed", "cycle", *(["--json"] if as_json else [])])
    output = capsys.readouterr().out
    assert code == 1
    if as_json:
        rec = json.loads(output)
        assert rec["stored"] == ["chat", "tickets"]
        assert rec["not_stored"] == ["chat", "tickets"]
    else:
        assert "stored 2 of 2" in output
        assert "coverage 0 of 2" in output


def test_406_01_unbound_invalid_input_does_not_poison_legacy_storage_check(clock):
    with pytest.raises(feed.FeedError):
        feed.add_lines("malformed")
    quiet("chat")
    assert feed.mark_cycle(["chat"])["not_stored"] == []
    assert health()["status"] == "missing", "legacy storage is still not declared setup"


def test_406_04_configuration_change_between_store_and_completion_needs_new_validation(clock):
    declare()
    quiet("tickets", "chat")
    config.set_value("feed_tool_sources", "mcp__replacement__*=tickets,mcp__chat__*=chat")
    got = feed.mark_cycle()
    assert got["not_stored"] == ["chat", "tickets"]
    assert health()["status"] == "unknown"


def test_406_10_existing_watcher_preserves_direct_source_provenance(clock):
    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
    feed.ingest([{"id": "page-1", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Resolved", "actor": "me", "status": "resolved"}])
    feed.mark_cycle()
    results.append(day_watch._src_evidence(results.today()))
    assert results.load()[0]["feed_source"] == "pagerduty"


def test_406_12_owner_stop_during_direct_publication_wins(clock, tmp_path, monkeypatch):
    live.start(json.dumps({"sources": [{"name": "pagerduty", "kind": "paging", "required": True, "inbox": str(tmp_path / "reply.json")}]}))
    standup.save_rows('{"rows":[{"title":"Incident","aliases":["incident"]}]}')
    feed.ingest([{"id": "page-1", "source": "pagerduty", "kind": "pagerduty", "at": "2026-11-05T11:55:00Z", "title": "Resolved incident", "actor": "me", "status": "resolved"}])
    original = standup.run

    def stop_during_publish(*args, **kwargs):
        live.stop()
        return original(*args, **kwargs)

    monkeypatch.setattr(standup, "run", stop_during_publish)
    feed.mark_cycle()
    assert not live._state()["running"]
    assert health()["status"] == "healthy" and health()["mode"] == "direct"


def test_406_01_declared_tools_without_a_batch_offer_direct_repair(clock):
    declare()
    got = health()
    assert got["status"] == "unknown"
    assert "careerbrew work feed cycle" in got["commands"]
    assert all("work live start" not in command for command in got["commands"])


def test_406_03_healthy_direct_status_does_not_request_adapter_setup(clock, capsys):
    declare()
    quiet("tickets", "chat")
    feed.mark_cycle()
    capsys.readouterr()
    assert cli.main(["work", "live", "status"]) == 0
    assert "no live sources" not in capsys.readouterr().out


def test_406_05_missing_adapter_ordering_clock_cannot_certify_old_success(clock, tmp_path):
    inbox = tmp_path / "reply.json"
    inbox.write_text("[]")
    at = datetime.fromisoformat("2026-11-05T12:00:00+00:00").timestamp()
    os.utime(inbox, (at, at))
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(inbox)}]}))
    live.cycle()
    quiet("tickets")
    feed.mark_cycle()
    st = live._state()
    st.pop("checked_at", None)
    live._save_state(st)
    assert health()["status"] == "unknown"


def test_406_05_failed_connector_overrides_only_its_pending_source(clock, monkeypatch):
    from careerbrew import connectors
    from careerbrew.work import evidence

    config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty,mcp__chat__*=slack")
    quiet("pagerduty", "slack")
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "pagerduty"}])

    def refused(_cfg):
        raise connectors.ConnectorError("synthetic provider refusal")

    monkeypatch.setattr(connectors, "feed", refused)
    assert feed.pull()["errors"] == 1
    assert feed.mark_cycle()["not_stored"] == ["pagerduty"]


def test_406_05_unusable_transcript_reply_overrides_pending_success(clock, tmp_path):
    from careerbrew.work import transcript

    declare()
    quiet("tickets", "chat")
    path = tmp_path / "session.jsonl"
    path.write_text(
        "\n".join(
            json.dumps(row)
            for row in [
                {"type": "response_item", "payload": {"type": "function_call", "name": "mcp__chat__search", "call_id": "broken", "arguments": "{}"}},
                {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "broken", "output": "unusable synthetic reply"}},
            ]
        )
    )
    assert "chat" in transcript.store(path)["gaps"]
    assert feed.mark_cycle()["not_stored"] == ["chat"]
