"""rust-07b: the pure decision functions decide from supplied data alone (no files, environment or network)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

from careerbrew import machine, msauth, plugins


def _facts(**over: Any) -> dict[str, Any]:
    return {"symlinks": [], "provides": {}, "live": {}, "terms_inside": True, **over}


def _no_steps(_steps: Any, _label: str) -> str | None:
    return None


def test_validate_manifest_data_accepts_a_minimal_manifest_with_no_files():
    data = {"name": "synthetic-co", "version": "0.1.0"}
    assert plugins.validate_manifest_data(data, _facts(), "1.0.0", _no_steps) is data


def test_validate_manifest_data_names_the_first_sorted_symlink():
    data = {"name": "synthetic-co", "version": "0.1.0"}
    with pytest.raises(plugins.PluginError, match=r"symlink \(a/link\)"):
        plugins.validate_manifest_data(data, _facts(symlinks=["a/link", "b/link"]), "1.0.0", _no_steps)


def test_manifest_facts_lists_symlinks_in_sorted_order(tmp_path: Path):
    (tmp_path / "target.txt").write_text("x", encoding="utf-8")
    for name in ("zz", "aa", "mm"):
        (tmp_path / name).symlink_to(tmp_path / "target.txt")
    assert plugins.manifest_facts({}, tmp_path)["symlinks"] == ["aa", "mm", "zz"]


def test_validate_manifest_data_uses_supplied_provides_facts():
    data = {"name": "synthetic-co", "version": "0.1.0", "provides": {"ladders": "ladders"}}
    missing = _facts(provides={"ladders": {"inside": True, "exists": False}})
    with pytest.raises(plugins.PluginError, match="missing path"):
        plugins.validate_manifest_data(data, missing, "1.0.0", _no_steps)
    outside = _facts(provides={"ladders": {"inside": False, "exists": True}})
    with pytest.raises(plugins.PluginError, match="escapes"):
        plugins.validate_manifest_data(data, outside, "1.0.0", _no_steps)
    present = _facts(provides={"ladders": {"inside": True, "exists": True}})
    assert plugins.validate_manifest_data(data, present, "1.0.0", _no_steps) is data


def test_validate_manifest_data_compares_the_supplied_version():
    data = {"name": "synthetic-co", "version": "0.1.0", "requires": ">=2.0.0"}
    with pytest.raises(plugins.PluginError, match="this is 1.9.9"):
        plugins.validate_manifest_data(data, _facts(), "1.9.9", _no_steps)
    assert plugins.validate_manifest_data(data, _facts(), "2.0.0", _no_steps) is data


def test_validate_manifest_data_uses_supplied_live_and_terms_facts():
    data = {"name": "synthetic-co", "version": "0.1.0", "live": {"team": "live/team.yaml"}}
    with pytest.raises(plugins.PluginError, match="live.team"):
        plugins.validate_manifest_data(data, _facts(live={"live/team.yaml": {"inside": True, "is_file": False}}), "1.0.0", _no_steps)
    terms = {"name": "synthetic-co", "version": "0.1.0", "sensitive_terms": "terms.txt"}
    with pytest.raises(plugins.PluginError, match="sensitive_terms"):
        plugins.validate_manifest_data(terms, _facts(terms_inside=False), "1.0.0", _no_steps)


def test_validate_manifest_data_raises_the_step_validators_refusal():
    data = {"name": "synthetic-co", "version": "0.1.0", "work_steps": ["bad"]}
    with pytest.raises(plugins.PluginError, match=r"synthetic step refusal \(fix:"):
        plugins.validate_manifest_data(data, _facts(), "1.0.0", lambda _s, _l: "synthetic step refusal")


def test_refresh_order_puts_bases_first_and_skips_unknown_and_cyclic_bases():
    lock = {"child": {"extends": "base"}, "base": {}, "orphan": {"extends": "absent"}, "x": {"extends": "y"}, "y": {"extends": "x"}}
    order = plugins.refresh_order(lock)
    assert order.index("base") < order.index("child")
    assert sorted(order) == ["base", "child", "orphan", "x", "y"]


def test_merge_policy_takes_the_strictest_values():
    manifests = [
        {"policy": {"allowed_hosts": ["a.example.com", "b.example.com"], "export_to_personal": "full", "agent_may_read_business": True}},
        {"policy": {"allowed_hosts": ["b.example.com"], "export_to_personal": "aggregates", "public_metrics": False, "backup_targets": ["~/synthetic-backup"]}},
    ]
    policy = plugins.merge_policy(manifests, {"~/synthetic-backup": "/synthetic-home/synthetic-backup"})
    assert policy["allowed_hosts"] == ["b.example.com"]
    assert policy["export_to_personal"] == "aggregates"
    assert policy["public_metrics"] is False
    assert policy["agent_may_read_business"] is True
    assert policy["backup_targets"] == ["/synthetic-home/synthetic-backup"]


def test_merge_policy_with_no_manifests_is_the_default():
    policy = plugins.merge_policy([], {})
    assert policy["allowed_hosts"] is None and policy["agent_may_read_business"] is False and policy["public_metrics"] is True


def test_guard_decision_refuses_personal_only_groups_on_corporate_only():
    assert machine.guard_decision("jobs", "corporate")
    assert machine.guard_decision("jobs", "personal") is None
    assert machine.guard_decision("day", "corporate") is None


def test_suggest_decision_reads_only_supplied_env_and_paths():
    detects = [{"env": ["SYNTHETIC_CORP_FLAG"], "paths": ["~/.synthetic-corp"]}]
    assert machine.suggest_decision(detects, {}, set()) is None
    assert machine.suggest_decision(detects, {"SYNTHETIC_CORP_FLAG": "1"}, set()) == "corporate"
    assert machine.suggest_decision(detects, {}, {"~/.synthetic-corp"}) == "corporate"


@pytest.mark.parametrize(
    ("parts", "icloud", "want"),
    [
        (("Library", "Mobile Documents", "x"), set(), "iCloud Drive"),
        (("Library", "CloudStorage", "SyntheticDrive-acct", "x"), set(), "SyntheticDrive"),
        (("Dropbox", "x"), set(), "Dropbox"),
        (("OneDrive - Synthetic", "x"), set(), "OneDrive - Synthetic"),
        (("Documents", "x"), {"Documents"}, "iCloud Desktop & Documents"),
        (("Documents", "x"), set(), ""),
        (("careerbrew-data",), set(), ""),
        ((), set(), ""),
    ],
)
def test_cloud_service_classifies_supplied_parts(parts, icloud, want):
    assert machine.cloud_service(parts, icloud) == want


def test_per_user_is_public_and_pure():
    assert machine.per_user({"email": "x"})
    assert machine.per_user({"path": "~/synthetic"})
    assert not machine.per_user({"path": "shared/synthetic"})


def _ready_facts(**over: Any) -> dict[str, Any]:
    base = {
        "role": "corporate",
        "profile_exists": True,
        "job_hunt_fields": False,
        "profile_name": "Synthetic Person",
        "profile_employer": "Synthetic Co",
        "personal_data": False,
        "installed": True,
        "verified": True,
        "real_ladder": True,
        "active_origin": "plugin:synthetic-co",
        "target_level": "senior",
        "active_levels": ["mid", "senior"],
        "cloud": "",
        "synced_targets": [],
        "shared": [],
        "terms_count": 2,
        "scan_ok": True,
        "files_scanned": 3,
        "scan_files": [],
    }
    return {**base, **over}


def test_readiness_rows_all_pass_on_ready_facts():
    rows = machine.readiness_rows(_ready_facts())
    assert len(rows) == 14 and all(r["ok"] and r["fix"] == "" for r in rows)


def test_readiness_rows_name_the_failed_facts():
    rows = machine.readiness_rows(_ready_facts(role="personal", cloud="Dropbox", target_level="staff"))
    failed = [r["check"] for r in rows if not r["ok"]]
    assert failed == [
        "machine role is corporate",
        "target_level 'staff' is a level of the active ladder",
        "the data folder is not uploaded by a cloud service (found: Dropbox)",
    ]


@pytest.mark.parametrize(
    ("answer", "want"),
    [
        ({"access_token": "a", "refresh_token": "r"}, "done"),
        ({"error": "authorization_pending"}, "wait"),
        ({"error": "slow_down"}, "slow"),
    ],
)
def test_classify_poll_verdicts(answer, want):
    assert msauth.classify_poll(answer) == want


def test_classify_poll_refusal_text():
    assert "you declined the sign-in" in msauth.classify_poll({"error": "authorization_declined"})
    assert "with no token" in msauth.classify_poll({})


def test_classify_refresh_verdicts():
    assert msauth.classify_refresh({"access_token": "a"}) == "token"
    ended = next(iter(msauth.ENDED))
    assert msauth.classify_refresh({"error": ended}) == "reconnect"
    assert "Microsoft refused the token (synthetic_refusal)" in msauth.classify_refresh({"error": "synthetic_refusal"})
