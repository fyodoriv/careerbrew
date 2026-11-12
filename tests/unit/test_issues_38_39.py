"""Issue #39 (no MEMORY.md as notes path) and #38 (plugin connector defaults, agent-read kinds, Enterprise token page)."""

from pathlib import Path

import pytest

from careerbrew import checkup, connectors, store
from careerbrew.cli import main
from careerbrew.work import projects

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"


def test_memory_index_is_refused_with_the_fix(home, tmp_path):
    with pytest.raises(projects.ProjectError, match="billing-research-log.md") as e:
        projects.add("billing", memory=str(tmp_path / "MEMORY.md"))
    assert "add this one line to the index" in str(e.value)
    projects.add("billing")
    with pytest.raises(projects.ProjectError, match="memory index"):
        projects.set_memory("billing", str(tmp_path / "notes" / "MEMORY.md"))


def test_a_folder_path_creates_the_research_log(home, tmp_path):
    (tmp_path / "notes").mkdir()
    projects.add("billing", memory=str(tmp_path / "notes"))
    log = tmp_path / "notes" / "billing-research-log.md"
    assert projects.show("billing")["memory"] == str(log) and log.read_text().startswith("# billing research log")


def test_checkup_flags_an_old_record_pointing_at_memory_md(home, tmp_path):
    projects.add("billing")
    data = store.read_yaml(projects.path())
    data["projects"]["billing"]["memory"] = str(tmp_path / "MEMORY.md")  # written before this check existed
    store.write_yaml(projects.path(), data)
    row = checkup.projects_step(refresh=False)
    assert not row["ok"] and "MEMORY.md" in row["step"]


def _plugin(tmp_path, body):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\n" + body)
    return root


def test_connector_defaults_fill_missing_flags_and_flags_win(home, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    root = _plugin(tmp_path, "connector_defaults:\n  jira: {base_url: 'https://jira.example.com', email: me@example.com}\n  github: {api: 'https://github.example.com/api/v3'}\n")
    assert main(["plugin", "add", str(root)]) == 0
    assert main(["connect", "jira", "--ref", "env:CB_T"]) == 0, "base_url and email come from the plugin"
    saved = {c["kind"]: c for c in store.read_yaml(home / "work" / "connectors.yaml")["connectors"]}
    assert saved["jira"]["base_url"] == "https://jira.example.com" and saved["jira"]["email"] == "me@example.com"
    capsys.readouterr()
    main(["connect", "github", "--ref", "env:CB_T", "--user", "me", "--no-verify"])
    assert "https://github.example.com/settings/personal-access-tokens/new" in capsys.readouterr().out
    main(["connect", "jira", "--ref", "env:CB_T", "--base-url", "https://scoped.atlassian.example.com", "--no-verify"])
    saved = {c["kind"]: c for c in store.read_yaml(home / "work" / "connectors.yaml")["connectors"]}
    assert saved["jira"]["base_url"] == "https://scoped.atlassian.example.com", "a flag wins over the default"


def test_token_page_follows_the_enterprise_api_and_plugin_override():
    assert connectors.token_page("github", "https://ghe.example.com/api/v3") == "https://ghe.example.com/settings/personal-access-tokens/new"
    assert connectors.token_page("github", "https://api.github.com") == connectors.GUIDE["github"]["token_page"]
    assert connectors.token_page("jira", None, {"token_page": "https://sso.example.com/tokens"}) == "https://sso.example.com/tokens"


def test_agent_read_kinds_are_labeled_not_failed(home, tmp_path):
    root = _plugin(tmp_path, "agent_reads: [slack]\n")
    assert main(["plugin", "add", str(root)]) == 0
    projects.add("billing")
    projects.add_source("billing", "chat", "#billing-eng")
    row = checkup.projects_step(refresh=False)
    assert row["ok"] and "agent reads itself" in row["step"]
    assert projects.reader("chat") == "agent" and projects.reader("tracker") == "careerbrew"


def test_bad_plugin_shapes_are_refused(home, tmp_path):
    from careerbrew import plugins

    with pytest.raises(plugins.PluginError, match="agent_reads"):
        plugins.add(str(_plugin(tmp_path, "agent_reads: slack\n")))
