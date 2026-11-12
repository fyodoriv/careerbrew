"""Draft save feedback identifies its local-only scope (#401)."""

import json
from unittest.mock import Mock

import pytest

from careerbrew import cli


def test_text_output_explains_local_save_and_next_step(home, tmp_path, capsys):
    draft = tmp_path / "draft.txt"
    draft.write_text("SYNTHCANARY reply")
    assert cli.main(["draft", "replace", "C9/1700000002.000100", "--file", str(draft)]) == 0
    output = capsys.readouterr().out
    assert "stored locally" in output
    assert "not saved in the chat tool" in output
    assert "save it there separately" in output
    assert "SYNTHCANARY" not in output


def test_json_output_exposes_scope_without_draft_text(home, tmp_path, capsys):
    draft = tmp_path / "draft.txt"
    draft.write_text("SYNTHCANARY reply")
    assert cli.main(["draft", "replace", "C9/1700000002.000100", "--file", str(draft), "--json"]) == 0
    output = capsys.readouterr().out
    result = json.loads(output)
    assert result["storage"] == "local"
    assert result["chat_tool_saved"] is False
    assert result["same"] is True
    assert "SYNTHCANARY" not in output


def test_failed_local_clear_names_local_store(home, tmp_path, capsys, monkeypatch):
    from careerbrew.work import drafts

    target = "C9/1700000002.000100"
    drafts.LocalBackend().save(target, "SYNTHCANARY old")
    monkeypatch.setattr(drafts.LocalBackend, "clear", lambda self, target: None)
    draft = tmp_path / "draft.txt"
    draft.write_text("SYNTHCANARY new")
    assert cli.main(["draft", "replace", target, "--file", str(draft)]) == 1
    output = capsys.readouterr().out
    assert "local draft store" in output
    assert "chat tool" not in output and "SYNTHCANARY" not in output
    backend = Mock()
    backend.read.return_value = "SYNTHCANARY old"
    with pytest.raises(drafts.DraftMismatch) as error:
        drafts.replace(target, "SYNTHCANARY new", backend=backend)
    assert "draft target" in str(error.value)
    assert "local draft store" not in str(error.value) and "SYNTHCANARY" not in str(error.value)


def test_local_readback_mismatch_names_local_store(home, tmp_path, capsys, monkeypatch):
    from careerbrew.work import drafts

    original = drafts.LocalBackend.save
    monkeypatch.setattr(drafts.LocalBackend, "save", lambda self, target, text: original(self, target, text[:3]))
    draft = tmp_path / "draft.txt"
    draft.write_text("SYNTHCANARY reply")
    assert cli.main(["draft", "replace", "C9/1700000002.000100", "--file", str(draft)]) == 1
    output = capsys.readouterr().out
    assert "local draft store" in output
    assert "chat tool" not in output and "SYNTHCANARY" not in output
    backend = Mock()
    backend.read.side_effect = [None, "other"]
    with pytest.raises(drafts.DraftMismatch) as error:
        drafts.replace("C9/1700000002.000100", "SYNTHCANARY reply", backend=backend)
    assert "draft target" in str(error.value)
    assert "local draft store" not in str(error.value) and "SYNTHCANARY" not in str(error.value)
