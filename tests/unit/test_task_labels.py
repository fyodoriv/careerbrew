"""Web payloads label tasks by title and project, not by raw id (issue #59)."""

import re

from careerbrew import server
from careerbrew.work import focus, projects

from .test_focus import _at

RAW_ID = re.compile(r"\bt\d{3}\b")


def _seed(home):
    from careerbrew.work import priorities

    t = priorities.add("write design doc for queue", 4, 4, 1, ["design"])
    for i in range(30):
        priorities.add(f"big ticket {i}", 2, 2, 6, [])
    projects.add("checkout")
    projects.link_task("checkout", t["id"])
    return t


def test_focus_titles_have_no_raw_ids_and_name_the_project(home, monkeypatch):
    t = _seed(home)
    d = focus.build(_at())
    items = [it for h in d["horizons"] for it in h["items"]]
    assert not [it["title"] for it in items if RAW_ID.search(it["title"])]
    mine = [it for it in items if it.get("ref") == t["id"] and it["title"] != items[0]["title"]]
    assert mine and all(it["project"] == "checkout" for it in mine)


def test_plans_and_tasks_carry_project_and_plain_labels(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", _at().isoformat(timespec="minutes"))
    t = _seed(home)
    today = server.today()
    row = next(s for s in today["plan"]["schedule"] if s["task"] == t["id"])
    assert row["project"] == "checkout"
    assert not RAW_ID.search(today["next"].get("plain", today["next"]["text"]))
    week = server.week(3)
    assert week["days"][-1]["unplaced"], "too much work: some tasks do not fit"
    assert not [x for x in week["days"][-1]["unplaced_titles"] if RAW_ID.search(x)]
    task = next(x for x in server.work()["tasks"] if x["id"] == t["id"])
    assert task["project"] == "checkout"
