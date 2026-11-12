"""Project approval notes remain distinct and respect restricted output (#429)."""

import json

import pytest

from careerbrew import cli
from careerbrew.work import projects


def test_approval_kind_is_saved_and_visible_in_project_list(home, capsys):
    projects.add("sample")
    assert cli.main(["work", "project", "update", "sample", "approval", "pending example PR"]) == 0
    capsys.readouterr()
    assert cli.main(["work", "project", "list", "--json"]) == 0
    (project,) = json.loads(capsys.readouterr().out)
    assert project["latest_update"]["kind"] == "approval"
    assert projects._load()["sample"]["updates"][-1]["text"] == "pending example PR"


def test_restricted_list_does_not_expose_approval_text(home):
    projects.add("sample")
    projects.record_update("sample", "approval", "SYNTHCANARY")
    (project,) = projects.watch_list()
    assert project["latest_update"]["kind"] == "approval"
    assert "SYNTHCANARY" not in json.dumps(project)


def test_unknown_update_kind_is_still_refused(home):
    projects.add("sample")
    with pytest.raises(projects.ProjectError, match="unknown update kind"):
        projects.record_update("sample", "invented", "example")
