"""Synthetic source-input boundary controls for evaluation."""

import importlib.util
from pathlib import Path

import pytest

from careerbrew import judge, paths

from .test_llm_evals import MODELS_YAML, FakeJudge, case


def runner():
    script = Path(__file__).resolve().parents[2] / "evals" / "run.py"
    spec = importlib.util.spec_from_file_location("synthetic_eval_input_runner", script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize("kind", ["case", "skill"])
def test_protected_eval_sources_are_refused(home, tmp_path, monkeypatch, kind):
    repo = tmp_path / "repo"
    cases = repo / "evals" / "cases"
    skills = repo / "skills" / "synthetic"
    cases.mkdir(parents=True)
    skills.mkdir(parents=True)
    private = home / "work" / "source"
    private.parent.mkdir(parents=True)
    private.write_text("id: synthetic\ninput: synthetic-private-canary\n" if kind == "case" else "synthetic-private-canary")
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    if kind == "case":
        (cases / "case.yaml").symlink_to(private)
    else:
        (skills / "SKILL.md").symlink_to(private)
    with pytest.raises(judge.LLMError):
        if kind == "case":
            module.load_cases()
        else:
            judge.answer_messages({"skill": "synthetic", "input": "synthetic"})
    assert "synthetic-private-canary" in private.read_text()


@pytest.mark.parametrize("name", ["../work", "/synthetic/absolute", "nested/name", "nested\\name", "..", ""])
def test_skill_names_cannot_escape_the_skill_folder(tmp_path, monkeypatch, name):
    repo = tmp_path / "repo"
    (repo / "skills").mkdir(parents=True)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    with pytest.raises(judge.LLMError, match="single skill name"):
        judge.answer_messages({"skill": name, "input": "synthetic"})


@pytest.mark.parametrize("kind", ["case", "skill"])
def test_external_source_symlinks_are_refused(home, tmp_path, monkeypatch, kind):
    repo = tmp_path / "repo"
    cases = repo / "evals" / "cases"
    skills = repo / "skills" / "synthetic"
    cases.mkdir(parents=True)
    skills.mkdir(parents=True)
    outside = tmp_path / "external-source"
    outside.write_text("id: synthetic\ninput: external-canary\n" if kind == "case" else "external-canary")
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    if kind == "case":
        (cases / "case.yaml").symlink_to(outside)
    else:
        (skills / "SKILL.md").symlink_to(outside)
    with pytest.raises(judge.LLMError, match="repository folder"):
        if kind == "case":
            module.load_cases()
        else:
            judge.answer_messages({"skill": "synthetic", "input": "synthetic"})


def test_in_repo_source_symlinks_work(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    cases = repo / "evals" / "cases"
    skills = repo / "skills" / "synthetic"
    cases.mkdir(parents=True)
    skills.mkdir(parents=True)
    (cases / "source.txt").write_text("id: synthetic\ninput: synthetic case\n")
    (cases / "case.yaml").symlink_to(cases / "source.txt")
    (skills / "source.txt").write_text("synthetic skill")
    (skills / "SKILL.md").symlink_to(skills / "source.txt")
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    assert module.load_cases()[0]["input"] == "synthetic case"
    assert "synthetic skill" in judge.answer_messages({"skill": "synthetic", "input": "synthetic"})[0]["content"]


def test_repo_inside_data_root_is_refused(home, monkeypatch):
    repo = home / "repo"
    (repo / "skills" / "synthetic").mkdir(parents=True)
    (repo / "skills" / "synthetic" / "SKILL.md").write_text("synthetic private canary")
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    with pytest.raises(judge.LLMError, match="data root"):
        judge.answer_messages({"skill": "synthetic", "input": "synthetic"})


def test_protected_skill_refuses_before_fake_model(home, tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    skills = repo / "skills" / "synthetic"
    skills.mkdir(parents=True)
    private = home / "work" / "skill"
    private.parent.mkdir(parents=True)
    private.write_text("synthetic-private-canary")
    (skills / "SKILL.md").symlink_to(private)
    config_dir = tmp_path / "config"
    config_dir.mkdir()
    (config_dir / "models.yaml").write_text(MODELS_YAML)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    monkeypatch.setattr(paths, "config_dir", lambda: config_dir)
    fake = FakeJudge()
    j = judge.Judge.from_config(transport=fake, cache=tmp_path / "cache")
    with pytest.raises(judge.LLMError):
        j.answer(case("synthetic") | {"skill": "synthetic"})
    assert fake.payloads == [] and not (tmp_path / "cache").exists()


@pytest.mark.parametrize("argv", [[], ["show"]])
def test_eval_cli_refuses_protected_case_without_content(home, tmp_path, monkeypatch, capsys, argv):
    repo = tmp_path / "repo"
    cases = repo / "evals" / "cases"
    (repo / "skills").mkdir(parents=True)
    cases.mkdir(parents=True)
    private = home / "work" / "case"
    private.parent.mkdir(parents=True)
    private.write_text("id: synthetic\ninput: synthetic-private-canary\n")
    (cases / "case.yaml").symlink_to(private)
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    monkeypatch.setenv("EVALS_LLM", "0")
    assert module.main(argv) == 1
    out = capsys.readouterr().out
    assert "FAIL  judge:" in out and "synthetic-private-canary" not in out


def test_missing_in_repo_skill_remains_empty(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    (repo / "skills").mkdir(parents=True)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    assert judge.skill_text("missing") == ""


def test_skill_final_entry_swap_refuses_before_model(home, tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    skills = repo / "skills" / "synthetic"
    skills.mkdir(parents=True)
    skill = skills / "SKILL.md"
    skill.write_text("synthetic external skill")
    private = home / "work" / "skill"
    private.parent.mkdir(parents=True)
    private.write_text("synthetic-private-canary")
    config_dir = tmp_path / "config"
    config_dir.mkdir()
    (config_dir / "models.yaml").write_text(MODELS_YAML)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    monkeypatch.setattr(paths, "config_dir", lambda: config_dir)
    fake = FakeJudge()
    j = judge.Judge.from_config(transport=fake, cache=tmp_path / "cache")
    original_open = judge.os.open

    def swap(path, flags, *args, **kwargs):
        skill.unlink()
        skill.symlink_to(private)
        return original_open(path, flags, *args, **kwargs)

    monkeypatch.setattr(judge.os, "open", swap)
    with pytest.raises(judge.LLMError, match="changed"):
        j.answer(case("synthetic") | {"skill": "synthetic"})
    assert fake.payloads == [] and private.read_text() == "synthetic-private-canary"


def test_eval_cli_refuses_protected_skill_with_model_off(home, tmp_path, monkeypatch, capsys):
    repo = tmp_path / "repo"
    skills = repo / "skills" / "synthetic"
    skills.mkdir(parents=True)
    (repo / "evals" / "cases").mkdir(parents=True)
    private = home / "work" / "skill"
    private.parent.mkdir(parents=True)
    private.write_text("synthetic-private-canary")
    (skills / "SKILL.md").symlink_to(private)
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    monkeypatch.setenv("EVALS_LLM", "0")
    assert module.main([]) == 1
    out = capsys.readouterr().out
    assert "FAIL  judge:" in out and "synthetic-private-canary" not in out


@pytest.mark.parametrize("kind", ["case", "skill"])
def test_external_swap_between_source_check_and_read_is_refused(home, tmp_path, monkeypatch, kind):
    repo = tmp_path / "repo"
    cases = repo / "evals" / "cases"
    skills = repo / "skills" / "synthetic"
    cases.mkdir(parents=True)
    skills.mkdir(parents=True)
    source = cases / "case.yaml" if kind == "case" else skills / "SKILL.md"
    source.write_text("id: synthetic\ninput: synthetic\n" if kind == "case" else "synthetic skill")
    outside = tmp_path / "external"
    outside.write_text("id: external\ninput: external-canary\n" if kind == "case" else "external-canary")
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    original = judge._read_resolved

    def swap(path):
        source.unlink()
        source.symlink_to(outside)
        return original(path)

    monkeypatch.setattr(judge, "_read_resolved", swap)
    with pytest.raises(judge.LLMError):
        if kind == "case":
            module.load_cases()
        else:
            judge.answer_messages({"skill": "synthetic", "input": "synthetic"})


@pytest.mark.parametrize("kind", ["case", "skill"])
def test_designated_source_directory_symlink_is_refused(tmp_path, monkeypatch, kind):
    repo = tmp_path / "repo"
    repo.mkdir()
    alternative = repo / "alternative"
    alternative.mkdir()
    module = runner()
    monkeypatch.setattr(module, "ROOT", repo)
    monkeypatch.setattr(paths, "REPO_ROOT", repo)
    if kind == "case":
        (repo / "evals").mkdir()
        (repo / "evals" / "cases").symlink_to(alternative, target_is_directory=True)
        (alternative / "case.yaml").write_text("id: synthetic\ninput: synthetic\n")
    else:
        (repo / "skills").mkdir()
        (repo / "skills" / "synthetic").symlink_to(alternative, target_is_directory=True)
        (alternative / "SKILL.md").write_text("synthetic skill")
    with pytest.raises(judge.LLMError, match="repository folder"):
        if kind == "case":
            module.load_cases()
        else:
            judge.skill_text("synthetic")
