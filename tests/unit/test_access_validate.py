"""rust-04: the access directory refuses malformed shapes with a fixed, content-free message (deny by default)."""

import json

import pytest

from careerbrew.cli import main
from careerbrew.work import access

CANARY = "CANARY-9b2e-synthetic-secret"
CH = {"id": "C1", "owns": ["vpn"], "members": ["U1"]}
TH = {"channel": "C1", "url": "https://chat.example.com/C1/p1", "text": "need vpn"}


def d(channels=None, threads=None):
    return {"channels": [CH] if channels is None else channels, "threads": [TH] if threads is None else threads}


BAD = [
    ({"channels": CANARY, "threads": []}, "directory: channels is not a list"),
    ({"channels": [CH], "threads": None}, "directory: threads is not a list"),
    (d(threads=CANARY), "directory: threads is not a list"),
    (d(channels=[CANARY]), "directory channel 0: not an object"),
    (d(channels=[{"owns": [CANARY]}]), "directory channel 0: missing id"),
    (d(channels=[{**CH, "owns": CANARY}]), "directory channel 0: owns is not a list of strings"),
    (d(channels=[{**CH, "owns": [1]}]), "directory channel 0: owns is not a list of strings"),
    (d(channels=[{**CH, "members": CANARY}]), "directory channel 0: members is not a list of strings"),
    (d(channels=[{**CH, "members": [None]}]), "directory channel 0: members is not a list of strings"),
    (d(channels=[CH, {**CH, "owns": [CANARY]}]), "directory channel 1: duplicate id"),
    (d(threads=[CANARY]), "directory thread 0: not an object"),
]


@pytest.mark.parametrize(("directory", "message"), BAD)
def test_each_malformed_shape_is_refused_with_a_fixed_message(directory, message):
    with pytest.raises(access.AccessError) as e:
        access.validate_directory(directory)
    assert str(e.value) == message and CANARY not in str(e.value)


def test_a_non_object_directory_is_refused():
    with pytest.raises(access.AccessError, match=r"^directory: not an object$"):
        access.validate_directory([CANARY])


@pytest.mark.parametrize(("directory", "message"), BAD)
def test_load_route_and_judge_refuse(tmp_path, directory, message):
    p = tmp_path / "dir.json"
    p.write_text(json.dumps(directory), encoding="utf-8")
    with pytest.raises(access.AccessError, match=f"^{message}$"):
        access.load(p)
    with pytest.raises(access.AccessError, match=f"^{message}$"):
        access.route("blocked: missing vpn permission", directory)
    with pytest.raises(access.AccessError, match=f"^{message}$"):
        access.judge("vpn please", "C1", directory)


def test_the_cli_prints_the_fixed_message_and_exits_nonzero(tmp_path, capsys):
    p = tmp_path / "dir.json"
    p.write_text(json.dumps(d(channels=[{**CH, "owns": CANARY}])), encoding="utf-8")
    draft = tmp_path / "draft.txt"
    draft.write_text("vpn please", encoding="utf-8")
    rc = main(["draft", "judge", "--draft", str(draft), "--channel", "C1", "--directory", str(p)])
    out = capsys.readouterr()
    assert rc != 0 and "directory channel 0: owns is not a list of strings" in out.out + out.err
    assert CANARY not in out.out + out.err and "Traceback" not in out.err


def test_valid_directories_behave_as_before(tmp_path):
    p = tmp_path / "dir.json"
    p.write_text(json.dumps({"channels": [CH]}), encoding="utf-8")
    assert access.load(p) == {"channels": [CH], "threads": []}
    assert access.route("blocked: missing vpn permission", d(), "U1") == {
        "channel": "C1",
        "topic": "vpn",
        "action": "link",
        "thread": TH["url"],
        "reason": "an open thread already asks this: link it, never open a second one",
        "tag": "in thread",
    }
    assert access.judge("vpn please", "C1", d()) == []
    assert access.judge("vpn please", "C9", d()) == ["channel C9 is not in the directory"]
    minimal = {"channels": [{"id": "C2"}]}
    access.validate_directory(minimal)
    assert access.route("blocked: missing vpn permission", minimal)["action"] == "unknown"
