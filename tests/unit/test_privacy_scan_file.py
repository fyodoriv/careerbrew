"""`privacy scan --file`: check a draft (an issue a checkup files) before it leaves the machine (issue #104)."""

import pytest

from careerbrew import privacy
from careerbrew.cli import main

from .conftest import CANARIES


def _draft(tmp_path, text, name="draft.md"):
    path = tmp_path / "drafts" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


def test_a_clean_draft_passes(home, ats_sources, tmp_path, capsys):
    draft = _draft(tmp_path, "## Problem\n`scripts/doctor.sh` printed a fix line for a script that does not exist.\n")
    assert main(["privacy", "scan", "--file", str(draft)]) == 0
    assert "1 file(s), 0 finding(s)" in capsys.readouterr().out


def test_a_sensitive_term_is_flagged_by_id_never_by_value(home, ats_sources, tmp_path, capsys):
    draft = _draft(tmp_path, f"## Problem\nevidence collect failed for the {CANARIES[1]} repo\n")
    assert main(["privacy", "scan", "--file", str(draft)]) == 1
    out = capsys.readouterr().out
    assert privacy.term_id(CANARIES[1]) in out and "1 finding(s)" in out
    assert CANARIES[1] not in out


def test_a_path_under_private_or_the_data_root_is_flagged(home, tmp_path):
    rel = _draft(tmp_path, "the error names private/work/evidence.jsonl\n", "rel.md")
    root = _draft(tmp_path, f"see {home}/day/log.md\n", "root.md")
    rep = privacy.scan_files([rel, root], terms=[])
    assert rep.findings == [(str(rel), "mentions-private-path"), (str(root), "mentions-data-root")]


@pytest.mark.parametrize(
    "text",
    [
        "see ./private/day/log.md",
        "in ~/apps/careerbrew/private/out/a.md",
        "(private/profile.yaml)",
        "`private/work/x`",
        "<repo>/private/work/evidence.jsonl",
        "${CAREERBREW_HOME}/private/x",
        "/private/work/evidence.jsonl",
        "<repo>/private/tmp/x",
        "$HOME/private/var/x",
    ],
)
def test_relative_and_nested_private_paths_are_flagged(home, tmp_path, text):
    rep = privacy.scan_files([_draft(tmp_path, text + "\n")], terms=[])
    assert [why for _, why in rep.findings] == ["mentions-private-path"]


def test_the_word_private_and_system_temp_paths_are_not_flagged(home, tmp_path):
    draft = _draft(tmp_path, "careerbrew never reads `private/`.\nThe log was at /private/tmp/run.log.\n/private/var/folders/x\n(/private/etc/hosts)\nmyprivate/x\n")
    assert privacy.scan_files([draft], terms=[]).ok


def test_the_web_app_and_agents_can_scan_two_drafts(home, ats_sources, tmp_path):
    from careerbrew import actions

    action = next(a for a in actions.catalog() if a["name"] == "privacy.scan")
    assert next(a for a in action["args"] if a["name"] == "files")["many"], "--file is repeatable, so the catalog takes a list"
    title = _draft(tmp_path, "checkup: the gate is red\n", "draft.title")
    body = _draft(tmp_path, "## Problem\nThe gate failed at step lint.\n", "draft.md")
    r = actions.run("privacy.scan", {"files": [str(title), str(body)]})
    assert r["argv"] == ["careerbrew", "privacy", "scan", f"--file={title}", f"--file={body}"]
    assert r["exit"] == 0 and "2 file(s), 0 finding(s)" in r["output"]


def test_a_file_inside_the_data_root_is_refused(home, tmp_path):
    inside = home / "out" / "draft.md"
    inside.parent.mkdir(parents=True, exist_ok=True)
    inside.write_text("nothing sensitive\n")
    assert privacy.scan_files([inside], terms=[]).findings == [(str(inside), "path-under-private")]


def test_several_files_are_scanned_and_counted(home, ats_sources, tmp_path, capsys):
    title = _draft(tmp_path, f"checkup: {CANARIES[0]} sees a red gate\n", "title.txt")
    body = _draft(tmp_path, "## Problem\nThe gate failed at step lint.\n", "body.md")
    assert main(["privacy", "scan", "--file", str(title), "--file", str(body)]) == 1
    out = capsys.readouterr().out
    assert "2 file(s), 1 finding(s)" in out and f"LEAK  {title}" in out


def test_a_missing_draft_is_an_error_not_a_pass(home, tmp_path, capsys):
    assert main(["privacy", "scan", "--file", str(tmp_path / "nope.md")]) == 2
    assert "nope.md" in capsys.readouterr().out


@pytest.mark.parametrize("flag", ["--staged", "--repo=."])
def test_file_cannot_be_mixed_with_a_repo_scan(home, tmp_path, flag, capsys):
    draft = _draft(tmp_path, "clean\n")
    assert main(["privacy", "scan", "--file", str(draft), flag]) == 2
    assert "--file" in capsys.readouterr().out


def test_a_finding_names_its_line_and_how_it_matched(home, ats_sources, tmp_path, capsys):
    draft = _draft(tmp_path, f"## Problem\nthe {CANARIES[1]} repo failed\n")
    assert main(["privacy", "scan", "--file", str(draft)]) == 1
    out = capsys.readouterr().out
    assert f"{draft}:2: {privacy.term_id(CANARIES[1])} (whole word)" in out
    assert "--show" in out and CANARIES[1] not in out


def test_letters_spread_across_words_say_it_is_likely_a_false_match(home, ats_sources, tmp_path, capsys):
    spread = " ".join(CANARIES[1])
    draft = _draft(tmp_path, f"one line\n{spread}\n")
    assert main(["privacy", "scan", "--file", str(draft)]) == 1
    out = capsys.readouterr().out
    assert f"{draft}:2:" in out and "spread across" in out and "likely a false match" in out


def test_show_prints_the_matched_text_to_the_terminal_only(home, ats_sources, tmp_path, capsys, monkeypatch):
    from careerbrew import cli

    shown = []
    monkeypatch.setattr(cli, "to_terminal", shown.append)
    draft = _draft(tmp_path, f"x\ny\nthe {CANARIES[1]} repo\n")
    assert main(["privacy", "scan", "--file", str(draft), "--show"]) == 1
    assert shown == [f"      line 3: {CANARIES[1]}"]
    assert CANARIES[1] not in capsys.readouterr().out


def test_a_match_over_a_line_break_is_still_found_and_says_so(home, ats_sources, tmp_path, capsys):
    term = CANARIES[1]
    draft = _draft(tmp_path, f"a\n{term[:4]}\n{term[4:]}\n")
    assert main(["privacy", "scan", "--file", str(draft)]) == 1
    out = capsys.readouterr().out
    assert f"{draft}:2:" in out and "spread over 2 lines" in out
