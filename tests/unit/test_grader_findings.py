"""Regression tests for defects found by the independent grader (2026-10-18)."""

import os

import pytest

from careerbrew import backup, goals, machine, plugins, review, secrets, store, sync
from careerbrew.cli import main
from careerbrew.jobs import sources, strategy
from careerbrew.work import evidence, promo


def test_plugin_remove_clears_derived_outputs_that_embed_plugin_terms(home, acme_plugin):
    plugins.add(str(acme_plugin), allow_exec=True)
    evidence.collect()
    out = promo.write_doc(promo.assess("generic-swe", "senior"))
    assert "PROJECTZEBRA" in out.read_text()
    plugins.remove("acme", purge_data=True)
    assert not out.exists(), "derived outputs must go with the plugin (its terms stop protecting them)"
    assert not (home / "cache").exists() or not any((home / "cache").iterdir())


def test_plugin_with_symlink_is_rejected(home, acme_plugin, tmp_path):
    secret = tmp_path / "outside.txt"
    secret.write_text("outside data")
    os.symlink(secret, acme_plugin / "rules-link.md")
    with pytest.raises(plugins.PluginError, match="symlink"):
        plugins.add(str(acme_plugin))


def test_backup_restore_refuses_job_hunt_data_on_corporate_and_never_restores_role(home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    (home / "tutor").mkdir()
    (home / "tutor" / "attempts.jsonl").write_text("{}\n")
    machine.set_role("personal")
    (cbk,) = backup.create()
    corp = tmp_path / "corp"
    os.environ["CAREERBREW_HOME"] = str(corp)
    machine.set_role("corporate")
    with pytest.raises(machine.RoleError):
        backup.restore(cbk, corp)
    assert not (corp / "tutor").exists()
    os.environ["CAREERBREW_HOME"] = str(home)
    dest = tmp_path / "pers"
    backup.restore(cbk, dest)
    assert not (dest / "machine.yaml").exists(), "a restore never carries a machine role"


def test_a_restore_on_a_corporate_machine_marks_a_fresh_folder_corporate(home, tmp_path):
    """The role lives in each data folder. A fresh folder with no role would run as personal: the guards would stop."""
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")
    store.write_text(home / "profile.yaml", "name: Sam Example\n")
    (cbk,) = backup.create()
    corp = tmp_path / "corp"
    os.environ["CAREERBREW_HOME"] = str(corp)
    machine.set_role("corporate", "work laptop")
    fresh = tmp_path / "fresh"
    backup.restore(cbk, fresh)
    os.environ["CAREERBREW_HOME"] = str(fresh)
    assert machine.role() == "corporate", "a folder restored on a corporate machine must not default to personal"
    os.environ["CAREERBREW_HOME"] = str(home)
    kept = tmp_path / "kept"
    kept.mkdir()
    store.write_yaml(kept / "machine.yaml", {"role": "corporate", "label": "its own"})
    backup.restore(cbk, kept)
    assert store.read_yaml(kept / "machine.yaml")["label"] == "its own", "a folder that has a role keeps it"
    assert not (tmp_path / "pers2").exists()
    backup.restore(cbk, tmp_path / "pers2")
    assert not (tmp_path / "pers2" / "machine.yaml").exists(), "a personal machine writes no role"


@pytest.mark.parametrize("kind", ["interview", "job-search"])
def test_a_corporate_machine_refuses_to_save_a_job_hunt_goal(home, kind):
    """The README promises that job-hunt data never sits on employer hardware. A goal with a company and a date is such data."""
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("corporate")
    for write in (lambda g: goals.set_upcoming(g), lambda g: goals.set_queue([g])):
        with pytest.raises(goals.GoalError, match="personal machine"):
            write({"kind": kind, "title": "Made-up loop", "company": "Example Co", "date": "2031-01-02"})
    assert not (home / "goals.yaml").exists(), "a refused goal writes nothing"
    assert goals.set_queue([{"kind": "promotion", "title": "Made-up case"}])["upcoming"]["kind"] == "promotion"


def test_a_machine_with_a_job_hunt_goal_cannot_become_corporate_until_it_is_removed(home):
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")
    goals.set_queue([{"kind": "interview", "title": "Made-up loop", "company": "Example Co"}, {"kind": "learn", "title": "Made-up topic"}])
    with pytest.raises(machine.RoleError, match="goals"):
        machine.set_role("corporate")
    assert machine.role() == "personal"
    goals.remove("made-up-loop")  # the removed goal moves to the history: that copy counts too
    with pytest.raises(machine.RoleError, match="goals"):
        machine.set_role("corporate")
    (home / "goals.yaml").unlink()  # the owner moves the file to the personal machine
    machine.set_role("corporate")
    assert goals.load()["queue"] == []


@pytest.mark.parametrize(
    ("rel", "text"),
    [
        ("profile.yaml", "name: Sam Example\ntarget_salary: 1\n"),
        ("profile.yaml", 'name: Sam Example\n"target_salary": 1\n'),
        ("profile.yaml", "{name: Sam Example, want_to_learn: [rust]}\n"),
        ("profile.yaml", "- not\n- a mapping\n"),
        ("profile.yaml", "null\n"),
        ("profile.yaml", "# a note\n~\n"),
        ("profile.yaml", "name: [unclosed\n"),
        ("out/strategy.md", "made-up plan\n"),
        ("out/reports/run-1.html", "<p>made-up</p>\n"),
        ("out/interview-acme.md", "made-up notes\n"),
        ("Jobs/tracker.yaml", "jobs: []\n"),
        ("goals.yaml", "queue:\n- {id: a, kind: interview, title: Made-up loop, company: Example Co}\n"),
        ("goals.yaml", "queue: []\nhistory:\n- {id: b, kind: job-search, title: Made-up search}\n"),
        ("goals.yaml", "- not\n- a mapping\n"),
    ],
)
def test_backup_restore_refuses_job_hunt_profile_fields_and_reports_on_corporate(home, tmp_path, rel, text):
    """The corporate role refuses these at setup (`machine.set_role`); a restore must not bring them back."""
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")
    store.write_text(home / rel, text)
    (cbk,) = backup.create()
    corp = tmp_path / "corp"
    os.environ["CAREERBREW_HOME"] = str(corp)
    machine.set_role("corporate")
    before = sorted(p.relative_to(corp) for p in corp.rglob("*"))
    with pytest.raises(machine.RoleError, match="job-hunt data"):
        backup.restore(cbk, corp)
    assert sorted(p.relative_to(corp) for p in corp.rglob("*")) == before, "a refused restore writes nothing"


def test_backup_restore_of_a_work_only_backup_passes_on_corporate(home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")
    store.write_text(home / "profile.yaml", "name: Sam Example\n")
    store.write_text(home / "out" / "standup.html", "<p>made-up</p>\n")
    (cbk,) = backup.create()
    corp = tmp_path / "corp"
    os.environ["CAREERBREW_HOME"] = str(corp)
    machine.set_role("corporate")
    assert backup.restore(cbk, tmp_path / "dest") >= 2
    assert store.read_text(tmp_path / "dest" / "profile.yaml") == "name: Sam Example\n"


@pytest.mark.parametrize("name", ["./jobs/tracker.yaml", "./machine.yaml", "work/../machine.yaml", "./profile.yaml", "/profile.yaml"])
def test_backup_restore_refuses_a_file_name_careerbrew_never_writes(home, tmp_path, monkeypatch, name):
    """`./jobs/x` is not in the job-hunt folder list by name, but it lands in `jobs/`."""
    import io
    import tarfile

    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")

    def odd_archive() -> bytes:
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tar:
            info = tarfile.TarInfo(name)
            info.size = 2
            tar.addfile(info, io.BytesIO(b"x\n"))
        return buf.getvalue()

    monkeypatch.setattr(backup, "_archive", odd_archive)
    (cbk,) = backup.create()
    dest = tmp_path / "dest"
    with pytest.raises(RuntimeError, match="never writes"):
        backup.restore(cbk, dest)
    assert not dest.exists()


def test_corporate_role_and_readiness_see_a_quoted_job_hunt_field(home):
    home.mkdir(parents=True, exist_ok=True)
    store.write_text(home / "profile.yaml", 'name: Sam Example\n"remote_preference": remote\n')
    with pytest.raises(machine.RoleError, match="job-hunt fields"):
        machine.set_role("corporate")
    assert machine.has_job_hunt_fields("{target_salary: 1}") and not machine.has_job_hunt_fields("name: Sam Example\n# target_salary: 1\n")
    assert not machine.has_job_hunt_fields("") and not machine.has_job_hunt_fields("# only a note\n")
    (row,) = [r for r in machine.readiness() if "job-hunt fields" in r["check"]]
    assert not row["ok"], "readiness sees the quoted field too"
    store.write_text(home / "profile.yaml", "name: Sam Example\n")
    (row,) = [r for r in machine.readiness() if "job-hunt fields" in r["check"]]
    assert row["ok"]


def test_backup_restore_refuses_a_link_member_and_keeps_a_two_dot_file_name(home, tmp_path, monkeypatch):
    """`alias -> .` then `alias/profile.yaml` would write the root profile past every name check."""
    import io
    import tarfile

    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("personal")
    store.write_text(home / "..notes.md", "made-up\n")
    os.symlink(home / "..notes.md", home / "link.md")
    (cbk,) = backup.create()
    assert backup.restore(cbk, tmp_path / "ok") >= 1, "a name that starts with two dots is a plain file name"
    assert (tmp_path / "ok" / "..notes.md").exists() and not (tmp_path / "ok" / "link.md").exists(), "a backup never carries a link"

    def link_archive() -> bytes:
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tar:
            link = tarfile.TarInfo("alias")
            link.type, link.linkname = tarfile.SYMTYPE, "."
            tar.addfile(link)
            info = tarfile.TarInfo("alias/profile.yaml")
            info.size = 2
            tar.addfile(info, io.BytesIO(b"x\n"))
        return buf.getvalue()

    os.link(home / "..notes.md", home / "twin.md")
    (twins,) = backup.create()
    assert backup.restore(twins, tmp_path / "twins") >= 2 and (tmp_path / "twins" / "twin.md").read_bytes() == (tmp_path / "twins" / "..notes.md").read_bytes(), "two hard-linked files restore as two files"

    trap = tmp_path / "trap"
    trap.mkdir()
    os.symlink(".", trap / "work")
    store.write_text(home / "work" / "note.md", "made-up\n")
    (deep,) = backup.create()
    with pytest.raises(RuntimeError, match="holds a link"):
        backup.restore(deep, trap)
    assert sorted(p.name for p in trap.iterdir()) == ["work"], "a destination with a link on the path gets nothing"

    real = tmp_path / "real"
    real.mkdir()
    os.symlink(".", real / "work")
    os.symlink(real, tmp_path / "via")
    with pytest.raises(RuntimeError, match="holds a link"):
        backup.restore(deep, tmp_path / "via")  # the destination is a link to a folder that holds the trap
    assert sorted(p.name for p in real.iterdir()) == ["work"]
    (real / "work").unlink()
    assert backup.restore(deep, tmp_path / "via") >= 1 and (real / "work" / "note.md").exists(), "a linked destination means its real folder"

    monkeypatch.setattr(backup, "_archive", link_archive)
    (bad,) = backup.create()
    with pytest.raises(RuntimeError, match="never writes"):
        backup.restore(bad, tmp_path / "bad")
    assert not (tmp_path / "bad").exists()


def test_keychain_store_keeps_token_out_of_argv(monkeypatch):
    seen = {}

    class R:
        returncode = 0
        stdout = ""

    def fake_run(cmd, **kw):
        seen["argv"] = cmd
        seen["input"] = kw.get("input", "")
        return R()

    monkeypatch.setattr(secrets.subprocess, "run", fake_run)
    secrets.store_keychain("careerbrew.slack", "tok-SECRET-123")
    assert "tok-SECRET-123" not in " ".join(seen["argv"])
    assert "tok-SECRET-123" in seen["input"]


def test_corporate_import_accepts_only_known_settings(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "pers"))
    machine.set_role("personal")
    (tmp_path / "pers" / "settings.yaml").write_text("ladder: generic-swe\ntarget_level: staff\nbackup_targets: [/tmp/evil]\n")
    sync.pair_init()
    b = tmp_path / "b.cbsync"
    sync.export(b)
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "corp"))
    machine.set_role("corporate")
    sync.import_bundle(b)
    text = store.read_text(tmp_path / "corp" / "settings.yaml")
    assert "staff" in text and "evil" not in text


def test_collect_reports_literal_token_reason(home, capsys):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text("connectors:\n  - {kind: slack, token: xoxp-literal}\n")
    main(["work", "collect"])
    main(["report", "latest", "--run", "work-collect"])
    out = capsys.readouterr().out
    assert "never a literal value" in out and "xoxp-literal" not in out
    assert "xoxp-literal" not in (home / "out" / "reports" / "latest.html").read_text(), "nor on the run page"


def test_strategy_skips_companies_whose_loop_is_used_and_folds_case(ats_sources):
    sources.scan()
    strategy.log_application("northwind", "target", "applied")
    strategy.log_application("northwind", "target", "rejected")
    plan = strategy.build()
    names = [r["company"].lower() for group in (plan["tiers"], plan["suggested"]) for rows in group.values() for r in rows]
    assert "northwind" not in names
    assert "Northwind" in plan["used_this_season"]


def test_dashboard_pivot_direction_for_lower_is_better(home):
    html = review.dashboard([{"metric": "estimate_ratio", "value": 3.0, "status": "PIVOT", "success": 1.3, "pivot": 2.0, "question": "", "action": "x", "higher_is_better": False}])
    assert "pivot &gt; 2.0" in html and "keep ≤ 1.3" in html


def test_json_outputs_are_redacted(ats_sources, capsys):
    (ats_sources / "sensitive-terms.txt").write_text("Northwind\n")
    sources.scan()
    for argv in (["jobs", "list", "--json"], ["jobs", "strategy", "--json"], ["drill", "next", "--json"]):
        main(argv)
        assert "northwind" not in capsys.readouterr().out.lower(), argv


def test_corporate_import_counts_only_applied_settings(tmp_path, monkeypatch):
    import json

    from cryptography.fernet import Fernet

    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "corp"))
    machine.set_role("corporate")
    key = sync.pair_init()
    b = tmp_path / "crafted.cbsync"
    b.write_bytes(Fernet(key.encode()).encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"settings": {"ladder": "generic-swe", "evil": 1, "jobs_mode": 2}}}).encode()))
    assert sync.import_bundle(b)["added"] == 1


def test_corporate_import_rejects_non_string_settings(tmp_path, monkeypatch):
    import json

    from cryptography.fernet import Fernet

    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "corp"))
    machine.set_role("corporate")
    key = sync.pair_init()
    b = tmp_path / "crafted.cbsync"
    b.write_bytes(Fernet(key.encode()).encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"settings": {"ladder": {"evil": [1]}, "target_level": "staff"}}}).encode()))
    assert sync.import_bundle(b)["added"] == 1
    assert "evil" not in store.read_text(tmp_path / "corp" / "settings.yaml")


def test_corporate_import_validates_ladder_and_level(tmp_path, monkeypatch):
    import json

    from cryptography.fernet import Fernet

    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "corp"))
    machine.set_role("corporate")
    key = sync.pair_init()

    def push(settings):
        b = tmp_path / "c.cbsync"
        b.write_bytes(Fernet(key.encode()).encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"settings": settings}}).encode()))
        return sync.import_bundle(b)["added"]

    assert push({"ladder": "no-such-ladder", "target_level": "DROP"}) == 0
    assert push({"ladder": "generic-swe", "target_level": "staff"}) == 2
    assert push({"target_level": "wizard"}) == 0
