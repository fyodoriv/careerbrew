"""Issue #311: each work step has one plain line that says what it does."""

import pytest

from careerbrew.workday import steps


def test_a_step_may_declare_about():
    assert steps.entry({"step": "/standup", "about": "Draft today's standup from yesterday's work"}, "settings")["about"] == "Draft today's standup from yesterday's work"
    assert steps.entry("/standup", "settings")["about"] == ""


@pytest.mark.parametrize("bad", [3, "", "a\nb", " ".join(["word"] * 16)])
def test_about_is_one_line_of_at_most_15_words(bad):
    with pytest.raises(steps.StepError, match="about"):
        steps.entry({"step": "/standup", "about": bad}, "settings")


def test_without_about_the_first_sentence_of_the_files_description_is_used(tmp_path):
    (tmp_path / "commands").mkdir()
    (tmp_path / "commands" / "standup.md").write_text('---\ndescription: "Draft the standup. It reads yesterday."\n---\nbody\n')
    assert steps.resolve("/standup", roots=[tmp_path])["description"] == "Draft the standup."
    assert steps.about({"about": ""}, {"description": "Draft the standup."}) == "Draft the standup."
    assert steps.about({"about": "Own words"}, {"description": "Draft the standup."}) == "Own words"


def test_a_file_without_description_gives_no_about(tmp_path):
    (tmp_path / "commands").mkdir()
    (tmp_path / "commands" / "x.md").write_text("no front matter\n")
    assert steps.resolve("/x", roots=[tmp_path])["description"] == ""


def test_declared_steps_carry_about(monkeypatch):
    monkeypatch.setattr(steps, "resolve", lambda step, roots=None, *, origin="": {"path": None, "user_only": False, "description": ""})
    from careerbrew import config

    config.set_value("work_steps", "/a, /b")
    got = {s["step"]: s["about"] for s in steps.declared()}
    assert got["/a"] == "" and "/careerbrew" in got
    assert steps.missing_about(steps.declared()) == ["/a", "/b", "/careerbrew"]
