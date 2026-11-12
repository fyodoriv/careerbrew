"""Issue #102: move per-person sources and connectors out of an existing company plugin, as they are."""

import secrets

import yaml

from careerbrew import actions, machine, paths, plugins, store
from careerbrew.cli import main
from careerbrew.work import evidence

# Synthetic values only; the plugin name is made at runtime (a fixed company name would trip the repo-term check).
SHARED = {"kind": "gh", "host": "github.example.com"}
REPO_A = {"kind": "git", "path": "/srv/repos/svc-a", "authors": ["owner@example.com"]}
HAND_PICKED = {"kind": "git", "path": "~/work/hand-picked"}  # no authors: the home path makes it one person's
JIRA = {"kind": "jira", "base_url": "https://gateway.example.com/jira", "jql": "project = ZZ AND assignee = currentUser()", "email": "owner@example.com", "token": "keychain:careerbrew.p-old.jira"}
SLACK = {"kind": "slack", "token": "keychain:careerbrew.slack"}


def _plugin(tmp_path, data):
    name = "zq" + secrets.token_hex(3)
    root = tmp_path / f"plugin-{name}"
    root.mkdir()
    (root / "evidence.yaml").write_text(yaml.safe_dump(data, sort_keys=False))
    (root / "careerbrew-plugin.yaml").write_text(f"name: {name}\nversion: 0.1.0\nprovides: {{evidence_sources: evidence.yaml}}\n")
    plugins.add(str(root))
    return name, root


def _mine(kind: str) -> list:
    return (store.read_yaml(paths.sub("work", f"{kind}.yaml"), {}) or {}).get(kind) or []


def test_moves_per_person_entries_as_they_are_and_keeps_shared_ones(tmp_path):
    store.write_yaml(paths.sub("work", "connectors.yaml"), {"connectors": [SLACK]})
    name, root = _plugin(tmp_path, {"sources": [REPO_A, HAND_PICKED, SHARED], "connectors": [JIRA]})
    total = len(evidence.sources_list())
    r = machine.split_plugin_evidence()
    assert r["plugins"] == [{"plugin": name, "file": "evidence.yaml", "before": 4, "after": 1, "sources": 2, "connectors": 1}]
    assert _mine("sources") == [REPO_A, HAND_PICKED], "hand-picked repos kept, with their authors"
    assert _mine("connectors") == [SLACK, JIRA], "the tuned gateway URL and JQL kept; my own connectors untouched"
    left = yaml.safe_load((root / "evidence.yaml").read_text())
    assert left["sources"] == [SHARED] and left["connectors"] == []
    assert r["total"] == (total, total) == (5, 5), "work collect reads the same sources before and after"
    assert r["mine"] == {"sources": (0, 2), "connectors": (1, 2)}
    assert r["left"] == [] and machine.per_user_values_in_plugins() == []
    assert all(ok for _, ok in plugins.verify()), "reinstalled from its folder, so the lock hash matches"
    row = next(x for x in machine.readiness() if "per-person values" in x["check"])
    assert row["ok"]


def test_dry_run_writes_nothing(tmp_path):
    _name, root = _plugin(tmp_path, {"sources": [REPO_A, SHARED], "connectors": [JIRA]})
    before = (root / "evidence.yaml").read_text()
    r = machine.split_plugin_evidence(dry_run=True)
    assert (r["plugins"][0]["sources"], r["plugins"][0]["connectors"]) == (1, 1)
    assert r["total"] == (3, 3) and r["mine"] == {"sources": (0, 1), "connectors": (0, 1)}, "the counts it would reach"
    assert (root / "evidence.yaml").read_text() == before
    assert not (paths.data_root() / "work" / "sources.yaml").exists()
    assert not (paths.data_root() / "work" / "connectors.yaml").exists()
    assert machine.per_user_values_in_plugins(), "still flagged until the real run"


def test_a_rerun_moves_nothing_and_never_duplicates(tmp_path):
    store.write_yaml(paths.sub("work", "sources.yaml"), {"sources": [REPO_A]})  # already mine, for example from new-user
    _plugin(tmp_path, {"sources": [REPO_A, HAND_PICKED]})
    first = machine.split_plugin_evidence()
    assert first["plugins"][0]["sources"] == 2 and first["mine"]["sources"] == (1, 2)
    assert first["total"] == (3, 2), "the same repo listed twice is read once after"
    again = machine.split_plugin_evidence()
    assert again["plugins"] == [] and again["total"] == (2, 2)
    assert _mine("sources") == [REPO_A, HAND_PICKED]


def test_a_plugin_without_its_folder_here_is_skipped_with_the_fix(tmp_path):
    name, root = _plugin(tmp_path, {"sources": [REPO_A]})
    root.rename(tmp_path / "moved-away")
    r = machine.split_plugin_evidence()
    assert r["plugins"][0]["plugin"] == name and "careerbrew plugin add <its folder>" in r["plugins"][0]["skipped"]
    assert not (paths.data_root() / "work" / "sources.yaml").exists(), "nothing moves when the plugin cannot be edited"
    assert r["left"] == [f"plugin:{name}:evidence.yaml"]


def test_command_sources_stay_in_the_plugin(tmp_path):
    cmd = {"kind": "command", "run": "bin/export", "user": "owner"}
    name, root = _plugin(tmp_path, {"sources": [cmd, REPO_A]})
    r = machine.split_plugin_evidence()
    assert yaml.safe_load((root / "evidence.yaml").read_text())["sources"] == [cmd], "a command runs inside its plugin"
    assert _mine("sources") == [REPO_A]
    assert r["left"] == [f"plugin:{name}:evidence.yaml"]


def test_cli_prints_the_counts_and_readiness_names_the_command(tmp_path, capsys):
    _plugin(tmp_path, {"sources": [REPO_A, HAND_PICKED, SHARED], "connectors": [JIRA]})
    row = next(x for x in machine.readiness() if "per-person values" in x["check"])
    assert not row["ok"] and "careerbrew machine split-plugin-evidence" in row["fix"]
    assert main(["machine", "split-plugin-evidence", "--dry-run"]) == 0
    out = capsys.readouterr().out
    assert "would move 2 source(s), 1 connector(s)" in out and "evidence sources work collect reads: 4 -> 4" in out
    assert main(["machine", "split-plugin-evidence"]) == 0
    out = capsys.readouterr().out
    assert "evidence.yaml: 4 evidence entries -> 1 shared (moved 2 source(s), 1 connector(s))" in out
    assert "work/sources.yaml: 0 -> 2 source(s); work/connectors.yaml: 0 -> 1 connector(s)" in out
    assert "ok    the company plugin evidence file holds no per-person values" in out
    assert "owner@example.com" not in out, "counts only, never the moved values"
    assert main(["machine", "split-plugin-evidence"]) == 0
    assert "nothing to move" in capsys.readouterr().out


def test_web_app_runs_it_only_after_confirm(tmp_path):
    action = next(a for a in actions.catalog() if a["name"] == "machine.split-plugin-evidence")
    assert action["confirm"] and action["ui"]
    r = actions.run("machine.split-plugin-evidence", {"dry_run": True}, confirm=True)
    assert r["exit"] == 0 and "nothing to move" in r["output"]
