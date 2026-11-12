"""Issue #267: a generic live feed in core with source adapters.

`work live start|cycle|status|stop` runs the cycle contract (lock, pull, store, triage, verdict per
event, actions, mark) over the sources in `work/live-sources.yaml` or a plugin's `live.sources`; a
required source that stored nothing fails the cycle (`not_stored`). A plugin supplies only its source
config and its roster. No plugin and no model call is needed: the fixtures are synthetic replies."""

import json
import os
from pathlib import Path

import pytest

from careerbrew import calendar, plugins, store
from careerbrew.work import feed, live, team

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "live"


@pytest.fixture
def inbox(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T16:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    d = tmp_path / "inbox"
    d.mkdir()
    live.start((FIX / "sources.yaml").read_text().replace("@DIR@", str(d)))
    return d


def _put(d: Path, name: str, text: str | None = None) -> None:
    path = d / f"{name}.json"
    path.write_text(text if text is not None else (FIX / f"{name}.json").read_text())
    at = calendar.now_utc().timestamp()
    os.utime(path, (at, at))


def test_a_cycle_that_stores_only_chat_returns_not_stored_with_the_other_required_source(inbox):
    _put(inbox, "chat")
    rec = live.cycle()
    assert rec["status"] == "failed"
    assert rec["not_stored"] == ["tickets"], "mail is optional, so only tickets"
    assert rec["sources"]["chat"]["new"] == 3 and rec["sources"]["chat"]["skipped"] == 1
    assert rec["actions"][0] == "pull tickets again, then run: careerbrew work live cycle"


def test_every_required_source_stored_passes_and_a_quiet_reply_counts_as_stored(inbox):
    _put(inbox, "chat")
    _put(inbox, "tickets", '{"tickets": []}')
    rec = live.cycle()
    assert rec["status"] == "ok" and rec["not_stored"] == []
    assert rec["sources"]["tickets"] == {**rec["sources"]["tickets"], "pulled": True, "stored": True, "items": 0}


def test_a_raw_ticket_search_reply_in_the_inbox_is_stored_as_the_feed_reads_it(inbox):
    # The search tool's own shape (key, fields.updated, fields.status), read by feed.raw_events (issue #254).
    raw = {"tickets": [{"key": "PROJ-9", "fields": {"summary": "Fix the login", "updated": "2026-10-25T15:00:00Z", "status": {"name": "In Progress"}}}]}
    _put(inbox, "chat")
    _put(inbox, "tickets", json.dumps(raw))
    rec = live.cycle()
    assert rec["status"] == "ok", rec
    assert rec["sources"]["tickets"]["new"] == 1 and rec["sources"]["tickets"]["skipped"] == 0
    assert any(r["id"] == "tickets:PROJ-9:2026-10-25T15:00:00Z:In Progress" for r in feed.load())


def test_a_stale_inbox_is_not_a_fresh_pull(inbox, monkeypatch):
    _put(inbox, "chat")
    _put(inbox, "tickets")
    assert live.cycle()["status"] == "ok"
    rec = live.cycle()
    assert rec["not_stored"] == [], "validated unchanged replies remain healthy within the acquisition limit"
    assert all(not rec["sources"][name]["pulled"] and not rec["sources"][name]["stored"] for name in ("chat", "tickets"))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T16:16")
    assert live.cycle()["not_stored"] == ["chat", "tickets"], "polling never renews acquisition"
    _put(inbox, "tickets")
    assert live.cycle()["not_stored"] == ["chat"]


def test_a_broken_inbox_fails_only_its_source_and_names_the_fix(inbox):
    _put(inbox, "chat", "not json at all")
    _put(inbox, "tickets")
    rec = live.cycle()
    assert rec["not_stored"] == ["chat"]
    assert "not JSON" in rec["sources"]["chat"]["error"]
    assert rec["sources"]["tickets"]["new"] == 1


def test_each_stored_event_gets_one_verdict_by_the_rules(inbox):
    store.write_yaml(feed.rules_path(), {"rules": [{"name": "shipped", "when": {"source": "tickets", "actor": "me"}, "evidence": True}]})
    _put(inbox, "chat")
    _put(inbox, "tickets")
    rec = live.cycle()
    assert rec["verdicts"] == {
        "chat:1792929600.000100": "answer",
        "chat:1792929660.000200": "skip",
        "chat:1792929720.000300": "read",
        "tickets:PROJ-7": "evidence",
    }
    assert any("1 item(s) reach the PM draft" in a for a in rec["actions"])
    assert any("answer 1 open ask(s)" in a for a in rec["actions"])


def test_the_pm_rule_needs_your_own_non_personal_evidence():
    row = {"id": "x", "triage": "mine", "evidence": True, "personal": False}
    assert live.reaches_pm(row)
    assert not live.reaches_pm({**row, "personal": True})
    assert not live.reaches_pm({**row, "evidence": False})
    assert live.verdict({**row, "personal": True}, set()) == "personal"
    assert live.verdict({**row, "triage": "act"}, set()) == "done", "an answered ask"


def test_cursors_move_only_for_sources_that_stored(inbox):
    _put(inbox, "tickets")
    live.cycle()
    st = live.status()
    since = {s["name"]: s["since"] for s in st["sources"]}
    assert since == {"chat": "", "tickets": "2026-10-21T14:20:00Z", "mail": ""}
    assert st["last"]["not_stored"] == ["chat"]


def test_the_pm_rule_agrees_with_the_pm_drafts_feed_input(inbox):
    store.write_yaml(feed.rules_path(), {"rules": [{"name": "shipped", "when": {"source": "tickets", "actor": "me"}, "evidence": True}, {"name": "all", "when": {"source": "chat"}, "evidence": True}]})
    _put(inbox, "chat")
    _put(inbox, "tickets")
    rec = live.cycle()
    assert {i for i, v in rec["verdicts"].items() if v == "evidence"} == {e["id"] for e in feed.evidence_items()} == {"tickets:PROJ-7"}, "a rule on someone else's message is no evidence"


def test_a_stop_during_a_cycle_is_kept(inbox, monkeypatch):
    _put(inbox, "tickets")
    real = live.mark_cycle

    def stop_then_mark(*args):
        live.stop()
        return real(*args)

    monkeypatch.setattr(live, "mark_cycle", stop_then_mark)
    live.cycle()
    assert live.status()["running"] is False
    assert live.status()["cycles"] == 1


def test_a_plugin_source_file_that_is_not_yaml_names_the_fix(inbox, tmp_path):
    root = _plugin(tmp_path, inbox)
    plugins.add(str(root))
    (Path(plugins.installed()[0]["root"]) / "live" / "sources.yaml").write_text("sources: [unclosed\n")
    with pytest.raises(live.LiveError, match="not YAML"):
        live.cycle()


def test_a_broken_plugin_roster_is_skipped_and_the_cycle_still_runs(inbox, tmp_path):
    plugins.add(str(_plugin(tmp_path, inbox)))
    (Path(plugins.installed()[0]["root"]) / "live" / "team.yaml").write_text("team: [unclosed\n")
    assert team.roster() == []
    _put(inbox, "chat")
    _put(inbox, "tickets")
    _put(inbox, "paging", "[]")
    assert live.cycle()["status"] == "ok"


def test_a_connector_error_is_hidden_from_a_piped_stdout(home, capsys, monkeypatch):
    from careerbrew import cli

    monkeypatch.setattr(live, "_read_connector", lambda s: (_ for _ in ()).throw(RuntimeError("host chat.example.com refused")))
    live.start("sources: [{name: chat, kind: chat, required: true, adapter: connector, connector: slack}]")
    assert cli.main(["work", "live", "cycle"]) == 1
    out = capsys.readouterr().out
    assert "chat.example.com" not in out and "RuntimeError (details hidden" in out


def test_one_cycle_at_a_time(inbox):
    with live.cycle_lock(), pytest.raises(live.LiveError, match="another live cycle is running"):
        live.cycle()


def test_stop_refuses_cycles_and_keeps_cursors(inbox):
    _put(inbox, "tickets")
    live.cycle()
    assert live.stop() == {"was_running": True, "cycles": 1}
    with pytest.raises(live.LiveError, match="stopped"):
        live.cycle()
    assert live.start()["sources"][1]["since"] == "2026-10-21T14:20:00Z"


@pytest.mark.parametrize(
    ("source", "error"),
    [
        ({"name": "Chat Room", "kind": "chat", "inbox": "x"}, "short lowercase name"),
        ({"name": "chat", "kind": "pager", "inbox": "x"}, "kind must be one of"),
        ({"name": "chat", "kind": "chat"}, "needs inbox"),
        ({"name": "chat", "kind": "chat", "adapter": "connector"}, "needs connector"),
        ({"name": "chat", "kind": "chat", "inbox": "x", "host": "y"}, "unknown key host"),
    ],
)
def test_a_bad_source_names_the_fix_and_saves_nothing(home, source, error):
    with pytest.raises(live.LiveError, match=error):
        live.save_sources(json.dumps({"sources": [source]}))
    assert live.sources() == []


def test_a_connector_source_that_is_not_connected_fails_its_source(home):
    live.start("sources: [{name: chat, kind: chat, required: true, adapter: connector, connector: slack}]")
    rec = live.cycle()
    assert rec["not_stored"] == ["chat"] and "careerbrew connect slack" in rec["sources"]["chat"]["error"]


def test_the_cycle_record_holds_no_message_text(inbox):
    _put(inbox, "chat")
    live.cycle()
    assert "LIVEZEBRA" not in (store.read_text(live.cycles_path()) or "")
    assert "LIVEZEBRA" not in (store.read_text(live.state_path()) or "")


def _plugin(tmp_path: Path, inbox: Path) -> Path:
    root = tmp_path / "plug"
    (root / "live").mkdir(parents=True)
    (root / "careerbrew-plugin.yaml").write_text("name: acme-live\nversion: 0.1.0\nlive: {sources: live/sources.yaml, team: live/team.yaml}\n")
    (root / "live" / "sources.yaml").write_text(f"sources:\n  - {{name: paging, kind: paging, required: true, adapter: file, inbox: {inbox}/paging.json}}\n  - {{name: chat, kind: chat, adapter: file, inbox: /nowhere.json}}\n")
    (root / "live" / "team.yaml").write_text("team:\n  - {name: Sam Lee, chat: U0SAM}\n")
    return root


def test_a_plugin_supplies_only_its_sources_and_roster(inbox, tmp_path):
    plugins.add(str(_plugin(tmp_path, inbox)))
    by = {s["name"]: s for s in live.sources()}
    assert by["paging"]["origin"] == "plugin:acme-live"
    assert by["chat"]["origin"] == "user", "the owner's source of the same name wins"
    assert [r["name"] for r in team.roster()] == ["Sam Lee"] and team.load() == [], "the plugin's roster is read only"
    _put(inbox, "paging", '[{"id": "P1", "at": "2026-10-25T15:58:00Z", "title": "disk full on db-1", "actor": "bot", "actor_id": "U0SAM"}]')
    _put(inbox, "chat")
    _put(inbox, "tickets")
    assert live.cycle()["status"] == "ok"
    row = next(r for r in feed.load() if r["id"] == "paging:P1")
    assert row["team"] == ["Sam Lee"] and row["origin"] == "plugin:acme-live"


@pytest.mark.parametrize(
    ("live_key", "error"),
    [("{bogus: x}", "live maps sources and team"), ("{sources: ../out.yaml}", "inside the plugin folder"), ("{team: missing.yaml}", "inside the plugin folder")],
)
def test_the_manifest_checks_live_files(tmp_path, live_key, error):
    root = tmp_path / "p"
    root.mkdir()
    (tmp_path / "out.yaml").write_text("sources: []\n")
    with pytest.raises(plugins.PluginError, match=error):
        plugins.validate_manifest({"name": "acme-live", "version": "1", "live": __import__("yaml").safe_load(live_key)}, root)


def test_the_fixture_runs_without_a_plugin(inbox):
    assert plugins.installed() == []
    for name in ("chat", "tickets", "mail"):
        _put(inbox, name)
    rec = live.cycle()
    assert rec["status"] == "ok" and sum(r["new"] for r in rec["sources"].values()) == 4
