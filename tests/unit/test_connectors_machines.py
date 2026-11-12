import json

import pytest

from careerbrew import connectors, machine, plugins, secrets
from careerbrew.cli import main
from careerbrew.work import evidence

from .conftest import CANARIES, FIXTURES

ALL = [
    {"kind": "github", "token": "env:CB_TEST_TOKEN", "user": "me"},
    {"kind": "jira", "token": "env:CB_TEST_TOKEN", "base_url": "https://jira.example.com", "email": "me@example.com"},
    {"kind": "slack", "token": "env:CB_TEST_TOKEN"},
    {"kind": "gdocs", "token": "env:CB_TEST_TOKEN"},
    {"kind": "zoom", "token": "env:CB_TEST_TOKEN"},
]


@pytest.fixture
def http(monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_TEST_TOKEN", "test-token-not-real")


def _write_connectors(home, conns):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text(json.dumps({"connectors": conns}))


def test_each_connector_maps_activity_to_tagged_evidence(home, http, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00+00:00")
    got = {c["kind"]: connectors.run(c, "user") for c in ALL}
    assert [i["title"] for i in got["github"]][:2] == ["feat: add CSV export to orders", "perf: cut checkout latency"]
    assert "Reviewed 1 pull requests" in [i["title"] for i in got["github"]]
    jira = {i["id"]: i["tags"] for i in got["jira"]}
    assert "quality" in jira["jira:PROJ-101"] and "impact" in jira["jira:PROJ-102"] and "ownership" in jira["jira:PROJ-103"]
    assert got["slack"][0]["tags"] == ["collaboration"]
    assert {i["id"]: i["tags"] for i in got["gdocs"]} == {"gdocs:doc1": ["design"], "gdocs:doc2": ["ownership"]}
    assert "leadership" in got["zoom"][0]["tags"] and "design" in got["zoom"][1]["tags"]


def test_slack_keeps_no_message_body(home, http):
    (item,) = connectors.run(ALL[2], "user")
    blob = json.dumps(item)
    assert CANARIES[2] not in blob and CANARIES[1] not in blob and "thanks for" not in blob


def test_collect_runs_connectors_and_dedups(home, http, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00+00:00")
    _write_connectors(home, ALL)
    first = evidence.collect()
    assert first["errors"] == 0 and first["new"] == 11  # github 2+1 reviews, jira 3, slack 1, gdocs 2, zoom 2
    assert evidence.collect()["new"] == 0


def test_literal_token_rejected(home, http):
    with pytest.raises(secrets.SecretError, match="reference"):
        connectors.run({"kind": "slack", "token": "xoxp-literal-token"}, "user")


def test_missing_secret_fails_with_fix(home, http, monkeypatch):
    monkeypatch.delenv("CB_TEST_TOKEN")
    with pytest.raises(secrets.SecretError, match="did not resolve"):
        connectors.run(ALL[3], "user")


def test_policy_limits_connectors_hosts_and_secret_backends(home, http, acme_plugin):
    m = acme_plugin / "careerbrew-plugin.yaml"
    m.write_text(
        m.read_text().replace(
            "  allowed_routes: [local]",
            "  allowed_routes: [local]\n  allowed_connectors: [jira, github]\n  allowed_hosts: [jira.example.com]\n  secret_backends: [keychain, op]",
        )
    )
    plugins.add(str(acme_plugin))
    with pytest.raises(connectors.ConnectorError, match="not allowed"):
        connectors.run(ALL[2], "user")  # slack blocked
    with pytest.raises(secrets.SecretError, match="not allowed"):
        connectors.run(ALL[1], "user")  # env: backend blocked
    with pytest.raises(connectors.ConnectorError, match="allowed_hosts"):
        connectors.http_json("https://api.github.com/search/issues", {})


def test_keychain_and_op_references_resolve_via_cli(monkeypatch):
    calls = []

    class R:
        returncode = 0
        stdout = "s3cret\n"

    def fake_run(cmd, **kw):
        calls.append(cmd[0])
        return R()

    monkeypatch.setattr(secrets.subprocess, "run", fake_run)
    assert secrets.resolve("keychain:careerbrew.jira") == "s3cret"
    assert secrets.resolve("op://Work/Jira/token") == "s3cret"
    assert calls == ["security", "op"]


def test_corporate_machine_blocks_job_hunt(home, capsys):
    machine.set_role("corporate")
    assert main(["jobs", "list"]) == 2
    assert "personal-only" in capsys.readouterr().out
    assert main(["drill", "list"]) == 2
    assert main(["work", "gaps"]) == 0


def test_machine_role_suggested_by_plugin_detect(home, acme_plugin, monkeypatch):
    plugins.add(str(acme_plugin))
    assert machine.suggest() is None
    monkeypatch.setenv("ACME_SSO_TOKEN", "x")
    assert machine.suggest() == "corporate"


# ---------- sync between corporate and personal ----------


def test_policy_export_level_takes_strictest(home, acme_plugin, tmp_path):
    import shutil

    a = acme_plugin / "careerbrew-plugin.yaml"
    a.write_text(a.read_text().replace("  allowed_routes: [local]", "  allowed_routes: [local]\n  export_to_personal: full"))
    plugins.add(str(acme_plugin))
    other = tmp_path / "other"
    shutil.copytree(acme_plugin, other)
    shutil.rmtree(other / "ladders")
    o = other / "careerbrew-plugin.yaml"
    o.write_text(o.read_text().replace("name: acme", "name: other").replace("  ladders: ladders/*.yaml\n", "").replace("export_to_personal: full", "export_to_personal: none"))
    plugins.add(str(other))
    assert plugins.effective_policy()["export_to_personal"] == "none"


def test_corporate_role_clears_examples_and_refuses_real_job_data(home):
    main(["init"])
    assert (home / "jobs" / "criteria.yaml").exists()
    machine.set_role("corporate")
    assert not (home / "jobs").exists()
    main(["init"])
    assert not (home / "jobs").exists(), "init on a corporate machine skips job-hunt files"
    machine.set_role("personal")
    main(["init"])
    (home / "jobs" / "ratings.jsonl").write_text('{"id": "x", "rating": 5}\n')
    with pytest.raises(machine.RoleError, match="job-hunt data"):
        machine.set_role("corporate")
