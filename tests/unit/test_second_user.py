"""Issue #48: a second person on a corporate machine with the shared company plugin."""

import pytest

from careerbrew import machine


def _plugin(tmp_path, evidence: str):
    root = tmp_path / "p"
    root.mkdir()
    (root / "evidence.yaml").write_text(evidence)
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nprovides: {evidence_sources: evidence.yaml}\n")
    return root


def test_readiness_flags_per_person_values_in_the_plugin(home, tmp_path):
    from careerbrew import plugins

    plugins.add(str(_plugin(tmp_path, "connectors:\n  - {kind: jira, base_url: 'https://jira.example.com', email: someone@example.com, token: 'keychain:x'}\n")))
    assert machine.per_user_values_in_plugins() == ["plugin:acme:evidence.yaml"]
    row = next(r for r in machine.readiness() if "per-person values" in r["check"])
    assert not row["ok"] and "machine new-user" in row["fix"]


def test_shared_only_plugin_passes(home, tmp_path):
    from careerbrew import plugins

    plugins.add(str(_plugin(tmp_path, "sources: []\n")))
    assert machine.per_user_values_in_plugins() == []


def test_find_repos_two_levels_down(tmp_path):
    for p in ("a/.git", "group/b/.git", "group/deep/x/c/.git", "notrepo"):
        (tmp_path / p).mkdir(parents=True)
    found = {p.name for p in machine.find_repos(tmp_path)}
    assert found == {"a", "b"}, "the folder and two levels below; deeper is out of reach"


def test_new_user_needs_a_plugin_and_an_email(home, tmp_path):
    with pytest.raises(ValueError, match="install the company plugin first"):
        machine.new_user(tmp_path, "N", "E", "me@example.com")
    from careerbrew import plugins

    plugins.add(str(_plugin(tmp_path, "sources: []\n")))
    with pytest.raises(ValueError, match="git config"):
        machine.new_user(tmp_path, "N", "E", "")
