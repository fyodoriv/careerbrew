"""Two sessions update two projects at once: both updates stay in projects.yaml (issue #230)."""

import subprocess
import sys
import time

from careerbrew.work import projects

# Each writer sleeps between its read and its write, so without a lock both start from the old file.
WRITER = """
import sys, time
from careerbrew.work import projects
load = projects._load
def slow():
    every = load()
    time.sleep(0.5)
    return every
projects._load = slow
projects.record_update(sys.argv[1], "project", "update from " + sys.argv[1])
projects.check(sys.argv[1], status="seen by " + sys.argv[1])
"""


def test_two_sessions_updating_two_projects_keep_both_updates(home):
    for name in ("billing", "search"):
        projects.add(name)
        projects.add_source(name, "tracker", f"PROJ-{len(name)}")
    procs = [subprocess.Popen([sys.executable, "-c", WRITER, n], stderr=subprocess.PIPE, text=True) for n in ("billing", "search")]
    errs = [p.communicate(timeout=60)[1] for p in procs]
    assert all(p.returncode == 0 for p in procs), errs
    every = projects._load()  # the stored record: `show` leaves updates out
    for name in ("billing", "search"):
        assert [u["text"] for u in every[name].get("updates", [])] == [f"update from {name}"], name
        assert [s["status"] for s in every[name]["sources"]] == [f"seen by {name}"], name


def test_a_nested_write_under_the_lock_does_not_wait_for_itself(home):
    with projects.locked():
        projects.add("billing")
        projects.record_update("billing", "team", "a new lead")
    assert projects._load()["billing"]["updates"][0]["text"] == "a new lead"


def test_an_import_waits_for_the_lock_and_rereads_the_file(home, tmp_path):
    projects.add("billing")
    sheet = tmp_path / "projects.csv"
    sheet.write_text("name,title\nsearch,Search\n")
    write = "import sys\nfrom careerbrew.work import project_import\nproject_import.run(sys.argv[1])\n"
    with projects.locked():
        p = subprocess.Popen([sys.executable, "-c", write, str(sheet)], stderr=subprocess.PIPE, text=True)
        every = projects._load()
        time.sleep(3)  # without the lock the import writes now, and the save below drops its row
        every["billing"]["title"] = "Billing"
        projects._save(every)
    assert p.wait(timeout=60) == 0, p.stderr.read()
    assert {r["name"] for r in projects.list_rows()} == {"billing", "search"}
    assert projects.show("billing")["title"] == "Billing"
