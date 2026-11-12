import json
import shutil
import subprocess

import pytest

from careerbrew import backup, events, models, plugins, privacy
from careerbrew.cli import main
from careerbrew.work import evidence, promo

from .conftest import CANARIES


def test_plugin_add_list_resources_policy_remove(home, acme_plugin):
    entry = plugins.add(str(acme_plugin))
    assert entry["origin"] == "plugin:acme" and not entry["exec_allowed"]
    assert "acme-eng" in promo.ladders()
    assert plugins.effective_policy()["allowed_routes"] == ["local"]
    assert plugins.verify() == [("acme", True)]
    plugins.remove("acme")
    assert "acme-eng" not in promo.ladders()
    assert plugins.effective_policy()["allowed_routes"] is None
    assert not (home / "plugins" / "acme").exists()


def test_plugin_tamper_detected(home, acme_plugin):
    plugins.add(str(acme_plugin))
    (home / "plugins" / "acme" / "rules.md").write_text("changed")
    assert plugins.verify() == [("acme", False)]


def test_plugin_rejects_bad_manifest_and_escape(home, acme_plugin):
    m = acme_plugin / "careerbrew-plugin.yaml"
    m.write_text(m.read_text().replace("rules: rules.md", "rules: ../../etc/passwd"))
    with pytest.raises(plugins.PluginError):
        plugins.add(str(acme_plugin))
    m.write_text("name: Bad Name\nversion: 1\n")
    with pytest.raises(plugins.PluginError):
        plugins.add(str(acme_plugin))


def test_plugin_conflict_on_ladder_id(home, acme_plugin, tmp_path):
    plugins.add(str(acme_plugin))
    twin = tmp_path / "twin"
    shutil.copytree(acme_plugin, twin)
    m = twin / "careerbrew-plugin.yaml"
    m.write_text(m.read_text().replace("name: acme", "name: acme-two"))
    with pytest.raises(plugins.PluginError, match="conflicts"):
        plugins.add(str(twin))


def test_command_source_needs_allow_exec_and_purge(home, acme_plugin):
    plugins.add(str(acme_plugin))
    assert evidence.collect()["errors"] == 1  # exec not allowed
    plugins.add(str(acme_plugin), allow_exec=True)
    assert evidence.collect()["new"] == 1
    assert evidence.load()[0]["origin"] == "plugin:acme"
    res = plugins.remove("acme", purge_data=True)
    assert res["purged_records"] == 1 and evidence.load() == []


def test_plugin_from_git_url(home, acme_plugin):
    subprocess.run(["git", "init", "-q", "-b", "main", str(acme_plugin)], check=True)
    subprocess.run(["git", "-C", str(acme_plugin), "add", "."], check=True)
    subprocess.run(
        ["git", "-C", str(acme_plugin), "-c", "user.email=t@example.com", "-c", "user.name=T", "-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", "-m", "init"],
        check=True,
    )
    entry = plugins.add(f"file://{acme_plugin}")
    assert entry["source"].startswith("file://")


# ---------- privacy ----------


def test_sensitive_terms_from_profile_user_and_plugin(ats_sources, acme_plugin):
    (ats_sources / "sensitive-terms.txt").write_text("# comment\nSecretCodename\n")
    plugins.add(str(acme_plugin))
    terms = privacy.sensitive_terms()
    for t in [*CANARIES, "SecretCodename", "AcmeSecretCustomer"]:
        assert t in terms


def test_redact_and_assert_clean(ats_sources):
    text = f"Hello {CANARIES[0]} at {CANARIES[1].lower()}"
    assert CANARIES[0] not in privacy.redact(text)
    with pytest.raises(privacy.PrivacyError) as exc:
        privacy.assert_clean(text, "test")
    assert CANARIES[0] not in str(exc.value), "error messages must not echo the term"


def test_route_guard_never_sends_private_data_to_public_routes(ats_sources, monkeypatch):
    monkeypatch.setenv("CAREERBREW_LLM", "on")
    sent = []

    def offline(payload, url):
        sent.append(url)
        raise OSError("offline")

    out, route = models.complete("summarize my evidence", privacy.BUSINESS, lambda: "fb", offline)
    assert route == "fallback" and out == "fb"
    assert sent and all("localhost" in u for u in sent), "business data may only try private_data routes"

    sent.clear()
    with pytest.raises(privacy.PrivacyError):
        # Public sensitivity, but the text holds a canary: blocked before any non-private route is called.
        models.complete(f"note about {CANARIES[1]}", privacy.PUBLIC, lambda: "fb", offline)
    assert all("localhost" in u for u in sent)


def test_plugin_policy_narrows_routes(home, acme_plugin):
    plugins.add(str(acme_plugin))
    assert [r.name for r in models.allowed_routes(privacy.PUBLIC)] == ["local"]


def test_events_are_content_free(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    events.emit("x", status="ok", command="jobs.scan", title=CANARIES[0], error="Has Spaces And Text", n=3)
    line = events.log_path().read_text()
    assert CANARIES[0] not in line and "Spaces" not in line
    assert json.loads(line)["n"] == 3


def test_repo_scan_flags_private_paths_and_terms(tmp_path, ats_sources):
    repo = tmp_path / "r"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    (repo / "ok.md").write_text("nothing here")
    (repo / "leak.md").write_text(f"met {CANARIES[1]} today")
    (repo / "private").mkdir()
    (repo / "private" / "x.yaml").write_text("a: 1")
    subprocess.run(["git", "-C", str(repo), "add", "-f", "."], check=True)
    rep = privacy.scan_repo(repo)
    assert {p for p, _ in rep.findings} == {"leak.md", "private/x.yaml"}


def test_backup_roundtrip_encrypted_and_tamper(home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text(f"name: {CANARIES[0]}\n")
    (written,) = backup.create()
    assert CANARIES[0].encode() not in written.read_bytes()
    assert backup.verify(written) >= 1
    assert backup.restore(written, tmp_path / "restored") >= 1
    assert CANARIES[0] in (tmp_path / "restored" / "profile.yaml").read_text()
    data = bytearray(written.read_bytes())
    data[50] ^= 1
    written.write_bytes(bytes(data))
    with pytest.raises(RuntimeError, match="failed verification"):
        backup.verify(written)


def test_backup_respects_plugin_target_policy(home, acme_plugin, tmp_path):
    m = acme_plugin / "careerbrew-plugin.yaml"
    m.write_text(m.read_text().replace("require_encrypted_backup: true", f"require_encrypted_backup: true\n  backup_targets: ['{tmp_path}/approved']"))
    plugins.add(str(acme_plugin))
    with pytest.raises(RuntimeError, match="no backup target"):
        backup.create()


def test_cli_show_refuses_business_content_when_piped(home, capsys):
    evidence.note("feat: shipped secret thing", ["delivery"])
    rc = main(["work", "promo", "--show"])
    out = capsys.readouterr().out
    assert rc == 3 and "shipped secret thing" not in out


def test_cli_output_is_redacted(ats_sources, capsys):
    main(["init"])
    assert CANARIES[0] not in capsys.readouterr().out


def test_backup_with_appended_bytes_fails_verification(home):
    home.mkdir(parents=True, exist_ok=True)
    (home / "a.txt").write_text("a")
    (written,) = backup.create()
    written.write_bytes(written.read_bytes() + b"x")
    with pytest.raises(RuntimeError, match="failed verification"):
        backup.verify(written)
