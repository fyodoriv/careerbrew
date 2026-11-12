import yaml

from careerbrew import plugins, privacy, wizard
from careerbrew.cli import main
from careerbrew.work import promo


def test_plugin_new_scaffolds_installs_and_lists_token_steps(home, tmp_path, capsys):
    rc = main(["plugin", "new", "Globex Corp", "--dir", str(tmp_path / "globex"), "--connect", "jira,github", "--jira-url", "https://jira.globex.example.com", "--github-user", "me", "--no-prompt"])
    out = capsys.readouterr().out
    pfx = wizard.service_prefix("globex-corp")
    assert rc == 0 and "globex" not in out.lower(), "the company name is a sensitive term"
    main(["report", "latest", "--run", "plugin-new"])
    out = capsys.readouterr().out
    assert "done     Created plugin [REDACTED] and installed it: at " in out
    assert f"-> careerbrew secret set {pfx}.jira" in out and f"-> careerbrew secret set {pfx}.github" in out
    m = yaml.safe_load((tmp_path / "globex" / "careerbrew-plugin.yaml").read_text())
    pol = m["policy"]
    assert pol["allowed_routes"] == ["local"] and pol["allowed_connectors"] == ["jira", "github"] and pol["export_to_personal"] == "none"
    assert "jira.globex.example.com" in pol["allowed_hosts"]
    plugin_text = "".join(f.read_text() for f in (tmp_path / "globex").rglob("*.yaml"))
    assert "me@" not in plugin_text and "github_user" not in plugin_text, "no per-person values in the shared plugin (issue #48)"
    assert m["connector_defaults"]["jira"]["base_url"] == "https://jira.globex.example.com"
    from careerbrew import paths, store

    mine = store.read_yaml(paths.data_root() / "work" / "connectors.yaml")["connectors"]
    assert {c["kind"] for c in mine} == {"jira", "github"} and all(c["token"].startswith(f"keychain:{pfx}.") for c in mine)
    assert "globex-corp-eng" in promo.ladders()
    assert "Globex Corp" in privacy.sensitive_terms()
    assert [p["name"] for p in plugins.installed()] == ["globex-corp"]
    assert (tmp_path / "globex" / ".git").exists()


def test_plugin_new_refuses_non_empty_dir_and_unknown_tool(home, tmp_path, capsys):
    (tmp_path / "x").mkdir()
    (tmp_path / "x" / "f").write_text("x")
    assert main(["plugin", "new", "Initech", "--dir", str(tmp_path / "x"), "--no-prompt"]) == 2
    assert main(["plugin", "new", "Initech", "--dir", str(tmp_path / "y"), "--connect", "myspace", "--no-prompt"]) == 2
    out = capsys.readouterr().out
    assert "not empty" in out and "unknown tools" in out


def test_secret_set_refuses_pipe(home, capsys):
    assert main(["secret", "set", "careerbrew.x"]) == 3


def test_plugin_new_on_corporate_role_writes_no_personal_host(home, tmp_path, capsys):
    assert main(["machine", "set", "corporate"]) == 0
    rc = main(["plugin", "new", "Globex Corp", "--dir", str(tmp_path / "g"), "--connect", "jira,github", "--jira-url", "https://jira.globex.example.com", "--github-api", "https://github.globex.example.com/api/v3", "--no-prompt"])
    assert rc == 0
    hosts = set(yaml.safe_load((tmp_path / "g" / "careerbrew-plugin.yaml").read_text())["policy"]["allowed_hosts"])
    assert not hosts & wizard.PERSONAL_HOSTS
    assert hosts == {"jira.globex.example.com", "github.globex.example.com"}, "only the hosts given by the flags"


def test_plugin_new_on_personal_role_keeps_personal_hosts(home, tmp_path, capsys):
    assert main(["plugin", "new", "Globex Corp", "--dir", str(tmp_path / "g"), "--connect", "jira", "--no-prompt"]) == 0
    hosts = set(yaml.safe_load((tmp_path / "g" / "careerbrew-plugin.yaml").read_text())["policy"]["allowed_hosts"])
    assert "api.fastmail.com" in hosts


def test_a_corporate_scaffold_keeps_numbers_out_of_public_text_by_default(home, tmp_path):
    corporate = yaml.safe_load((wizard.scaffold("Initech", tmp_path / "corp", [], {}, role="corporate") / "careerbrew-plugin.yaml").read_text())
    personal = yaml.safe_load((wizard.scaffold("Initech", tmp_path / "mine", [], {}) / "careerbrew-plugin.yaml").read_text())
    assert corporate["policy"]["public_metrics"] is False, "an employer's numbers go public only after the policy file says so"
    assert personal["policy"]["public_metrics"] is True
