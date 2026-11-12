"""Ownership contract (#115): careerbrew owns career data and its workflows; agentbrew only deploys them."""

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
DOCS = ["AGENTS.md", "README.md", "docs/plugins.md"]
LITMUS = "Litmus test: if it reads or writes the user's career data, it is careerbrew."
DEPLOY_ONLY = "agentbrew only deploys careerbrew's commands; it never owns them."
OWNED = ["day log", "PM update", "standup", "evidence", "promotion docs", "work feed", "web views"]
OWNS_LINE = re.compile(r"^.*careerbrew owns (?:the user's|your) work record and everything built from it.*$", re.M)


def _text(doc: str) -> str:
    return (ROOT / doc).read_text()


@pytest.mark.parametrize("doc", DOCS)
def test_doc_carries_the_one_line_litmus_test(doc):
    assert LITMUS in _text(doc)


@pytest.mark.parametrize("doc", DOCS)
def test_doc_says_careerbrew_owns_the_work_record_and_what_is_built_from_it(doc):
    match = OWNS_LINE.search(_text(doc))
    assert match, f"{doc}: no 'careerbrew owns ... work record and everything built from it' line"
    missing = [item for item in OWNED if item not in match.group(0)]
    assert not missing, f"{doc}: the ownership line misses {missing}"


@pytest.mark.parametrize("doc", DOCS)
def test_doc_says_agentbrew_only_deploys_careerbrew_commands(doc):
    assert DEPLOY_ONLY in _text(doc)


def test_plugin_commands_folder_is_the_source_of_truth_not_a_hand_copy():
    text = _text("docs/plugins.md")
    assert "A plugin's `commands/` folder is the source of truth for its slash commands" in text
    assert "never a hand copy in an agent config folder" in text
    assert "careerbrew plugin refresh" in text
