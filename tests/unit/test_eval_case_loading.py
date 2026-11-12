"""Eval document integrity through the actual loader, without model calls."""

import runpy
from pathlib import Path

import pytest
import yaml
from yaml.constructor import ConstructorError

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def loader():
    return runpy.run_path(str(ROOT / "evals/run.py"))["load_cases"]


def synthetic(loader, monkeypatch, tmp_path, text):
    from careerbrew import paths

    cases = tmp_path / "evals/cases"
    cases.mkdir(parents=True)
    path = cases / "synthetic.yaml"
    path.write_text(text)
    monkeypatch.setitem(loader.__globals__, "ROOT", tmp_path)
    monkeypatch.setattr(paths, "REPO_ROOT", tmp_path.resolve())
    return path


def test_duplicate_id_is_refused(loader, monkeypatch, tmp_path):
    path = synthetic(loader, monkeypatch, tmp_path, "id: first\nid: second\ninput: synthetic\n")
    with pytest.raises(ConstructorError) as error:
        loader()
    assert error.value.problem_mark.name == str(path)
    assert error.value.problem_mark.line == 1


def test_duplicate_expect_is_refused(loader, monkeypatch, tmp_path):
    synthetic(loader, monkeypatch, tmp_path, "id: first\nexpect: [first criterion]\nexpect: [second criterion]\n")
    with pytest.raises(ConstructorError):
        loader()


def test_nested_duplicate_key_is_refused(loader, monkeypatch, tmp_path):
    synthetic(loader, monkeypatch, tmp_path, "id: first\nmetadata:\n  key: first\n  key: second\n")
    with pytest.raises(ConstructorError):
        loader()


def test_separate_documents_keep_repeated_field_names(loader, monkeypatch, tmp_path):
    synthetic(loader, monkeypatch, tmp_path, "id: first\ninput: first request\nexpect: [first criterion]\n---\nid: second\ninput: second request\nexpect: [second criterion]\n")
    assert loader() == [
        {"id": "first", "input": "first request", "expect": ["first criterion"]},
        {"id": "second", "input": "second request", "expect": ["second criterion"]},
    ]


@pytest.mark.parametrize(
    "text",
    [
        "defaults: &base {a: 1}\nitem: {<<: *base, a: 2}\n",
        "first: &first {a: 1, b: 2}\nsecond: &second {a: 3, c: 4}\nitem: {<<: [*first, *second], b: 5}\n",
    ],
)
def test_merge_overrides_keep_safe_loader_semantics(loader, monkeypatch, tmp_path, text):
    synthetic(loader, monkeypatch, tmp_path, text)
    assert loader() == [yaml.safe_load(text)]


def test_duplicate_explicit_key_after_merge_is_refused(loader, monkeypatch, tmp_path):
    synthetic(loader, monkeypatch, tmp_path, "base: &base {a: 1}\nitem: {<<: *base, a: 2, a: 3}\n")
    with pytest.raises(ConstructorError):
        loader()


def test_unsafe_tag_is_refused(loader, monkeypatch, tmp_path):
    synthetic(loader, monkeypatch, tmp_path, "item: !!python/object/apply:os.system ['echo should-not-run']\n")
    with pytest.raises(ConstructorError):
        loader()


def test_real_live_feed_cases_remain_complete_and_separate(loader):
    wanted = [
        {
            "id": "live-feed-pulls-in-one-parallel-batch",
            "suite": "regression",
            "skill": "careerbrew-live-feed",
            "should": "act",
            "input": "run one live cycle; sources are chat, tickets, and calendar, plus two teammates",
            "expect": [
                "Sends every source and teammate search in one batch of parallel tool calls, not one after another, then runs one `careerbrew work live cycle`",
                "Bounds the ticket search with a relative cursor such as `updated >= -45m`, never a clock time without a zone",
            ],
        },
        {
            "id": "live-feed-standup-failed-is-not-a-failed-cycle",
            "suite": "regression",
            "skill": "careerbrew-live-feed",
            "should": "act",
            "input": "the live cycle printed `standup: failed (StandupError); the cycle goes on` and no not_stored line",
            "expect": [
                "Treats the cycle as ok, not failed: does not pull the sources again",
                "Tells the user once that the standup publish failed, and suggests `careerbrew work standup` by hand if it repeats",
            ],
        },
    ]
    assert [case for case in loader() if case.get("id") in {item["id"] for item in wanted}] == wanted
