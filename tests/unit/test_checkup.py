"""`careerbrew checkup` (issue #8) and the venv architecture check (issue #4)."""

from careerbrew import checkup
from careerbrew.cli import main


def _page_text(capsys) -> str:
    """The checkup's run page as text (`careerbrew report latest`): one done or blocker line per step."""
    capsys.readouterr()
    main(["report", "latest", "--run", "checkup"])
    return capsys.readouterr().out


def test_checkup_reports_one_line_per_step(home, capsys):
    rc = main(["checkup"])
    out = capsys.readouterr().out.splitlines()
    assert out[-1].startswith("checkup: ") and rc in (0, 1)
    steps = [line for line in _page_text(capsys).splitlines() if line.startswith(("done     ", "blocker  "))]
    assert len(steps) == len(checkup.STEPS)
    assert any("promo coverage" in s and "first run" in s for s in steps)
    main(["checkup"])
    assert "since last checkup" in _page_text(capsys)


def test_arch_mismatch_fails_with_a_fix():
    bad = checkup.arch_step(hw="arm64", py="x86_64")
    assert not bad["ok"] and "aarch64" in bad["fix"]
    assert checkup.arch_step(hw="arm64", py="arm64")["ok"]


def test_plugin_checkup_step_needs_allow_exec(home, tmp_path, capsys):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text('name: acme\nversion: 0.1.0\ncheckup: [{name: vpn, run: ["true"]}]\n')
    main(["plugin", "add", str(root)])
    main(["checkup"])
    assert "acme: vpn (skipped" in _page_text(capsys)
    main(["plugin", "add", str(root), "--allow-exec"])
    main(["checkup"])
    assert "done     acme: vpn" in _page_text(capsys)


def test_pull_only_fails_for_a_pushable_local_path(home, monkeypatch, tmp_path):
    import subprocess

    from careerbrew import machine

    machine.set_role("corporate", "")
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: subprocess.CompletedProcess(a, 0, f"{tmp_path}\n", ""))
    assert not checkup.pull_only_step()["ok"]
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: subprocess.CompletedProcess(a, 0, "DISABLED\n", ""))
    assert checkup.pull_only_step()["ok"]


def test_offline_skips_the_network_collect(home, monkeypatch, capsys):
    from careerbrew.work import evidence

    monkeypatch.setattr(evidence, "collect", lambda: {"new": 0, "total": 0, "errors": 1})
    assert main(["checkup"]) == 1
    capsys.readouterr()
    main(["checkup", "--offline"])
    out = _page_text(capsys)
    assert "evidence collect (skipped: --offline)" in out and "source error" not in out


def test_pull_only_accepts_the_runbook_value(home, monkeypatch):
    import subprocess

    from careerbrew import machine

    machine.set_role("corporate", "")
    for url, ok in (("no-push-from-corporate", True), ("git@github.com:o/r.git", False), ("https://github.com/o/r", False)):
        monkeypatch.setattr(subprocess, "run", lambda *a, u=url, **k: subprocess.CompletedProcess(a, 0, u + "\n", ""))
        assert checkup.pull_only_step()["ok"] is ok, url


def test_web_step_flags_a_missing_build(monkeypatch):
    from careerbrew import server

    monkeypatch.setattr(server, "dist_exists", lambda: False)
    row = checkup.web_step()
    assert not row["ok"] and "npm run build" in row["fix"]
