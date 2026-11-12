"""Synthetic controls for evaluation paths at the data-root boundary."""

import importlib.util
import os
from pathlib import Path

import pytest

from careerbrew import judge, paths

from .test_llm_evals import MODELS_YAML, FakeJudge, case


@pytest.fixture
def evals_env(tmp_path, monkeypatch):
    monkeypatch.delenv("EVALS_LLM_URL", raising=False)
    config_dir = tmp_path / "eval-config"
    config_dir.mkdir()
    (config_dir / "models.yaml").write_text(MODELS_YAML)
    monkeypatch.setattr(paths, "config_dir", lambda: config_dir)
    return tmp_path


def test_protected_cache_is_refused_before_model_or_write(home, evals_env):
    fake = FakeJudge()
    protected = home / "work" / "eval-cache"
    j = judge.Judge.from_config(transport=fake, cache=protected)
    with pytest.raises(judge.LLMError, match="data root"):
        j.answer(case("synthetic"))
    assert fake.payloads == []
    assert not protected.exists()


def test_protected_labels_are_refused_before_write(home):
    protected = home / "work" / "labels.jsonl"
    with pytest.raises(judge.LLMError, match="data root"):
        judge.add_label(case("synthetic"), "synthetic answer", "pass", path=protected)
    assert not protected.exists()


@pytest.mark.parametrize("folder", ["work", "out", "logs", "plugins", ".edit", "learn/demo/replay/123"])
def test_data_root_classes_are_refused(home, folder):
    target = home / folder / "labels.jsonl"
    target.parent.mkdir(parents=True, exist_ok=True)
    if folder.startswith("learn/"):
        git = target.parent / ".git"
        git.mkdir()
        (git / "HEAD").write_text("synthetic")
    target.write_text("synthetic private canary")
    with pytest.raises(judge.LLMError, match="data root"):
        judge.read_labels(target)
    assert target.read_text() == "synthetic private canary"


@pytest.mark.parametrize("encryption", ["on", "off"])
@pytest.mark.parametrize("link", [False, True])
def test_protected_label_read_refuses_before_content_access(home, tmp_path, monkeypatch, encryption, link):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", encryption)
    protected = home / "work" / "labels.jsonl"
    protected.parent.mkdir(parents=True)
    sentinel = b'{"answer":"synthetic private canary"}\n'
    protected.write_bytes(sentinel)
    target = protected
    if link:
        target = tmp_path / "labels-link"
        target.symlink_to(protected)
    with pytest.raises(judge.LLMError, match="data root") as exc:
        judge.read_labels(target)
    assert "synthetic private canary" not in str(exc.value)
    assert protected.read_bytes() == sentinel


def test_cache_file_symlink_is_refused_without_read_or_transport(home, tmp_path, evals_env):
    protected = home / "work" / "secret.json"
    protected.parent.mkdir(parents=True)
    sentinel = b'{"text":"synthetic private canary"}'
    protected.write_bytes(sentinel)
    cache = tmp_path / "outside-cache"
    cache.mkdir()
    fake = FakeJudge()
    j = judge.Judge.from_config(transport=fake, cache=cache)
    sample = case("synthetic")
    payload = j._payload(j.answer_route, judge.answer_messages(sample), judge.ANSWER_TOKENS)
    j._path(j.answer_route, payload).symlink_to(protected)
    with pytest.raises(judge.LLMError, match="data root"):
        j.answer(sample)
    assert fake.payloads == [] and protected.read_bytes() == sentinel


def test_allowed_external_cache_and_labels_keep_readback(tmp_path, evals_env):
    fake = FakeJudge()
    j = judge.Judge.from_config(transport=fake, cache=tmp_path / "external-cache")
    sample = case("synthetic")
    answer = j.answer(sample)
    assert j.answer(sample) == answer and len(fake.payloads) == 1
    label = tmp_path / "external-labels.jsonl"
    judge.add_label(sample, answer, "pass", path=label)
    rows, problems = judge.read_labels(label)
    assert not problems and rows[0]["answer"] == answer


def test_final_entry_swap_during_read_is_refused(home, tmp_path, monkeypatch):
    outside = tmp_path / "outside.json"
    outside.write_text("synthetic external")
    protected = home / "work" / "protected.json"
    protected.parent.mkdir(parents=True)
    protected.write_text("synthetic private canary")
    real_open = judge.os.open

    def swap(path, flags, *args, **kwargs):
        outside.unlink()
        outside.symlink_to(protected)
        return real_open(path, flags, *args, **kwargs)

    monkeypatch.setattr(judge.os, "open", swap)
    with pytest.raises(judge.LLMError, match="changed"):
        judge._read(outside)
    assert protected.read_text() == "synthetic private canary"


def test_replace_failure_keeps_old_labels_and_cleans_temp(tmp_path, monkeypatch):
    target = tmp_path / "labels.jsonl"
    target.write_text("synthetic original")
    monkeypatch.setattr(judge.os, "replace", lambda *args: (_ for _ in ()).throw(OSError("synthetic failure")))
    with pytest.raises(OSError):
        judge._write(target, "synthetic replacement")
    assert target.read_text() == "synthetic original"
    assert not list(tmp_path.glob(".eval-*.tmp"))


def _runner():
    path = Path(__file__).resolve().parents[2] / "evals" / "run.py"
    spec = importlib.util.spec_from_file_location("synthetic_eval_runner", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize("argv", [["show"], ["label", "synthetic", "pass"], ["agreement"], []])
def test_eval_commands_refuse_protected_labels_without_content(home, monkeypatch, capsys, argv):
    target = home / "work" / "labels.jsonl"
    target.parent.mkdir(parents=True)
    target.write_text("synthetic private canary")
    monkeypatch.setenv("EVALS_LABELS", str(target))
    monkeypatch.setenv("EVALS_LLM", "0")
    runner = _runner()
    monkeypatch.setattr(runner, "load_cases", lambda: [])
    assert runner.main(argv) == 1
    out = capsys.readouterr().out
    assert "data root" in out and "synthetic private canary" not in out
    assert target.read_text() == "synthetic private canary"


@pytest.mark.parametrize("termination", ["normal", "exception", "exit"])
def test_synthetic_case_restores_caller_environment(home, tmp_path, monkeypatch, termination):
    from careerbrew import cli

    monkeypatch.setenv("EVALS_LABELS", str(tmp_path / "labels.jsonl"))
    monkeypatch.setenv("SYNTHETIC_KEEP", "before")
    monkeypatch.delenv("SYNTHETIC_ABSENT", raising=False)
    runner = _runner()
    monkeypatch.setattr(runner, "load_cases", lambda: [{"id": "synthetic", "skill": "none", "should": "act", "cli": ["fixture"], "expect_regex": "ok", "env": {"SYNTHETIC_KEEP": "during", "SYNTHETIC_ABSENT": "during"}}])
    seen = []

    def fake_cli(argv):
        seen.append((os.environ["SYNTHETIC_KEEP"], os.environ["SYNTHETIC_ABSENT"], os.environ["CAREERBREW_HOME"]))
        if termination == "exception":
            raise RuntimeError("synthetic")
        if termination == "exit":
            raise SystemExit(7)
        print("ok")

    monkeypatch.setattr(cli, "main", fake_cli)
    before = dict(os.environ)
    if termination == "normal":
        runner.run_all()
    else:
        with pytest.raises(RuntimeError if termination == "exception" else SystemExit):
            runner.run_all()
    assert dict(os.environ) == before
    assert seen and seen[0][:2] == ("during", "during") and seen[0][2] != str(home)


@pytest.mark.parametrize("key", ["CAREERBREW_HOME", "CAREERBREW_EVENTS", "CAREERBREW_LLM", "CAREERBREW_KEY_FILE", "CAREERBREW_BACKUP_DIRS", "CAREERBREW_PAIR_KEY_FILE"])
def test_case_cannot_override_synthetic_safety_setting(monkeypatch, tmp_path, key, capsys):
    from careerbrew import cli

    monkeypatch.setenv("EVALS_LABELS", str(tmp_path / "labels.jsonl"))
    runner = _runner()
    monkeypatch.setattr(runner, "load_cases", lambda: [{"id": "synthetic", "skill": "none", "should": "act", "cli": ["fixture"], "expect_regex": "ok", "env": {key: "synthetic"}}])
    calls = []
    monkeypatch.setattr(cli, "main", lambda argv: calls.append(argv))
    assert runner.run_all() == 1 and calls == []
    assert "cannot override synthetic safety settings" in capsys.readouterr().out


def test_protected_labels_after_synthetic_case_use_restored_root(home, monkeypatch, capsys):
    from careerbrew import cli

    target = home / "work" / "labels.jsonl"
    target.parent.mkdir(parents=True)
    target.write_text("synthetic private canary")
    monkeypatch.setenv("EVALS_LABELS", str(target))
    monkeypatch.setenv("EVALS_LLM", "0")
    runner = _runner()
    monkeypatch.setattr(runner, "load_cases", lambda: [{"id": "synthetic", "skill": "none", "should": "act", "cli": ["fixture"], "expect_regex": "ok"}])
    calls = []

    def fake_cli(argv):
        calls.append(argv)
        print("ok")

    monkeypatch.setattr(cli, "main", fake_cli)
    before = dict(os.environ)
    assert runner.run_all() == 1 and calls == [["fixture"]]
    assert dict(os.environ) == before
    out = capsys.readouterr().out
    assert "data root" in out and "synthetic private canary" not in out
    assert target.read_text() == "synthetic private canary"


def test_cache_directory_swap_after_model_does_not_write_protected(home, tmp_path, evals_env):
    protected = home / "work"
    protected.mkdir(parents=True)
    outside = tmp_path / "outside"
    outside.mkdir()
    link = tmp_path / "cache-link"
    link.symlink_to(outside, target_is_directory=True)

    def transport(payload, url):
        link.unlink()
        link.symlink_to(protected, target_is_directory=True)
        return "synthetic response"

    j = judge.Judge.from_config(transport=transport, cache=link)
    with pytest.raises(judge.LLMError, match="data root"):
        j.answer(case("synthetic"))
    assert list(protected.iterdir()) == []


def test_unsafe_cache_refuses_before_cli_show_model(home, tmp_path, monkeypatch, evals_env, capsys):
    labels = tmp_path / "labels.jsonl"
    monkeypatch.setenv("EVALS_LABELS", str(labels))
    fake = FakeJudge()
    j = judge.Judge.from_config(transport=fake, cache=home / "work" / "cache")
    runner = _runner()
    monkeypatch.setattr(runner, "load_cases", lambda: [case("synthetic")])
    monkeypatch.setattr(judge.Judge, "from_config", lambda: j)
    assert runner.main(["show", "synthetic"]) == 1
    assert "data root" in capsys.readouterr().out and fake.payloads == []


def test_missing_nofollow_fails_closed(tmp_path, monkeypatch):
    target = tmp_path / "external"
    target.write_text("synthetic")
    monkeypatch.delattr(judge.os, "O_NOFOLLOW")
    with pytest.raises(judge.LLMError, match="unavailable"):
        judge._read(target)


def test_empty_data_root_fails_closed(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", "")
    with pytest.raises(judge.LLMError, match="safely resolve"):
        judge._read(tmp_path / "external")


def test_parent_swap_before_mkdir_creates_no_protected_directory(home, tmp_path, monkeypatch):
    protected = home / "work"
    protected.mkdir(parents=True)
    outside = tmp_path / "outside"
    outside.mkdir()
    link = tmp_path / "switch"
    link.symlink_to(outside, target_is_directory=True)
    original = judge._checked_path
    calls = 0

    def swap_after_check(path):
        nonlocal calls
        resolved = original(path)
        calls += 1
        if calls == 1:
            link.unlink()
            link.symlink_to(protected, target_is_directory=True)
        return resolved

    monkeypatch.setattr(judge, "_checked_path", swap_after_check)
    with pytest.raises(judge.LLMError, match="data root"):
        judge._write(link / "nested" / "labels.jsonl", "synthetic")
    assert list(protected.iterdir()) == []
