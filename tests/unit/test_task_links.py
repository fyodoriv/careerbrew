"""A task's links and its own why reach every task row in the web app (issue #143, US-70)."""

import json

import pytest

from careerbrew import events, server, store
from careerbrew.cli import main
from careerbrew.day import log
from careerbrew.work import focus, priorities, project_priority

from .test_focus import _at

FILE = "https://github.example.com/acme/shop/blob/main/src/app.py"
TICKET = "https://jira.example.com/browse/SHOP-12"
CHANNEL = "https://slack.example.com/archives/C0CHECKOUT"


@pytest.mark.parametrize(
    ("value", "label", "url", "web"),
    [
        (FILE, "", FILE, True),
        (f"ticket={TICKET}", "ticket", TICKET, True),
        (f"post in #checkout={CHANNEL}", "post in #checkout", CHANNEL, True),
        ("https://x.example.com/search?q=a=b", "", "https://x.example.com/search?q=a=b", True),  # a query `=` is not a label
        ("https://x.example.com/r?next=https://y.example.com/a", "", "https://x.example.com/r?next=https://y.example.com/a", True),  # a redirect URL stays whole
        ("https://sso.example.com/login?return_to=http://y.example.com/a", "", "https://sso.example.com/login?return_to=http://y.example.com/a", True),
        ("sign in=https://x.example.com/r?next=https://y.example.com/a", "sign in", "https://x.example.com/r?next=https://y.example.com/a", True),
        ("HTTPS://X.EXAMPLE.COM/a", "", "HTTPS://X.EXAMPLE.COM/a", True),
        ("javascript:alert(1)", "", "javascript:alert(1)", False),
        ("click=javascript:alert(1)", "", "click=javascript:alert(1)", False),
        ("data:text/html,<b>x</b>", "", "data:text/html,<b>x</b>", False),
        ("file:///etc/hosts", "", "file:///etc/hosts", False),
        ("https://", "", "https://", False),
        ("src/app.py", "", "src/app.py", False),
    ],
)
def test_a_link_is_url_or_label_equals_url_and_only_http_is_web(value, label, url, web):
    assert priorities.parse_link(value) == {"label": label, "url": url}
    assert priorities.is_web(url) is web


def test_link_repeats_keeps_labels_and_warns_on_a_value_that_is_not_a_full_url(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")
    assert main(["work", "task", "Tell the repo owner about the file", "--why", f"seen in {TICKET}", "--link", FILE, "--link", f"ticket={TICKET}", "--link", "#checkout"]) == 0
    out = capsys.readouterr().out
    assert "warning: --link '#checkout' is not a full http(s):// URL" in out and "added t001" in out
    assert out.count("warning") == 1, "a full URL, labeled or not, gives no warning"
    (t,) = priorities.load()
    assert t["links"] == [{"label": "", "url": FILE}, {"label": "ticket", "url": TICKET}, {"label": "", "url": "#checkout"}]
    assert "link" not in t
    assert priorities.links(t) == [{"label": "github.example.com/…/app.py", "url": FILE}, {"label": "ticket", "url": TICKET}, {"label": "#checkout", "url": "#checkout"}]


def test_a_task_keeps_at_most_eight_links_and_says_so(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")
    many = [arg for i in range(10) for arg in ("--link", f"https://x.example.com/{i}")]
    assert main(["work", "task", "Many links", *many]) == 0
    assert "the last 2 were left out" in capsys.readouterr().out
    assert len(priorities.load()[0]["links"]) == priorities.MAX_LINKS


def _legacy(home, link=FILE):
    """A tasks.yaml written before issue #143: one `link` string."""
    store.write_yaml(
        priorities.tasks_path(), {"tasks": [{"id": "t001", "title": "Ping the owner about a file", "impact": 4, "urgency": 4, "estimate_h": 0.5, "competencies": [], "due": None, "status": "open", "source": "user", "why": "", "link": link}]}
    )


def test_an_old_task_with_one_link_reads_as_links_and_a_save_keeps_the_new_shape(home):
    _legacy(home)
    (t,) = priorities.load()
    assert t["links"] == [{"label": "", "url": FILE}] and "link" not in t
    assert [x["links"] for x in priorities.rank({})] == [[{"label": "", "url": FILE}]]
    priorities.done("t001", 1)
    stored = store.read_yaml(priorities.tasks_path(), {})["tasks"][0]
    assert stored["links"] == [{"label": "", "url": FILE}] and "link" not in stored, "the next save writes the migrated shape"


def test_an_old_task_with_an_empty_link_has_no_links(home):
    _legacy(home, link="")
    assert priorities.load()[0].get("links") is None
    assert priorities.links(priorities.load()[0]) == []


def test_today_week_next_step_and_dashboard_rows_carry_links_and_the_tasks_own_why(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", _at().isoformat(timespec="minutes"))
    t = priorities.add("Tell the repo owner about the file", 5, 5, 0.5, [], why=f"the owner asked in {CHANNEL}", links=[FILE, f"ticket={TICKET}"])
    want = {"links": [{"label": "github.example.com/…/app.py", "url": FILE}, {"label": "ticket", "url": TICKET}], "task_why": f"the owner asked in {CHANNEL}"}
    today = server.today()
    row = next(s for s in today["plan"]["schedule"] if s["task"] == t["id"])
    assert {k: row[k] for k in want} == want
    assert today["next"]["task"] == t["id"] and {k: today["next"][k] for k in want} == want
    week_rows = [s for d in server.week(7)["days"] for s in d["schedule"] if s["task"] == t["id"]]
    assert week_rows and all({k: s[k] for k in want} == want for s in week_rows)
    d = focus.build(_at())
    rows = [it for h in d["horizons"] for it in h["items"] if it.get("ref") == t["id"]]
    assert {h["id"] for h in d["horizons"] if any(it.get("ref") == t["id"] for it in h["items"])} >= {"now", "today"}
    assert rows and all({k: it[k] for k in want} == want for it in rows)
    assert all("score" in it["why"] for it in rows), "the computed score line stays in `why`"


def test_an_old_tasks_link_reaches_the_web_rows_too(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", _at().isoformat(timespec="minutes"))
    _legacy(home)
    row = next(s for s in server.today()["plan"]["schedule"] if s["task"] == "t001")
    assert row["links"] == [{"label": "github.example.com/…/app.py", "url": FILE}] and row["task_why"] == ""


def test_the_day_log_shows_the_first_web_link_and_never_a_script_link(home):
    _legacy(home, link="javascript:alert(1)")
    t = priorities.load()[0]
    assert log._plan_detail(t, {}, "planned", [])["url"] == "", "a javascript: link is never a source link"
    t2 = priorities.add("Review the export", 3, 3, 1, [], links=["notes.md", TICKET])
    detail = log._plan_detail(t2, {}, "planned", [])
    assert detail["url"] == TICKET and "SHOP-12" in detail["related"]


def test_adding_a_task_logs_link_counts_and_never_a_url(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    priorities.add("Ping CANARY-EMPLOYER-3X about the file", 3, 3, 1, [], why="asked in chat", links=[FILE, "#checkout"])
    raw = events.log_path().read_text()
    (e,) = [r for r in map(json.loads, raw.splitlines()) if r["event"] == "work.task"]
    assert (e["status"], e["source_kind"], e["links"], e["web_links"], e["story"]) == ("open", "user", 2, 1, "us-70")
    assert "example.com" not in raw and "CANARY" not in raw and "checkout" not in raw


def test_project_priority_reads_every_link_of_a_task(home, monkeypatch):
    t = priorities.add("Polish the sync output", 3, 3, 1, [], links=[TICKET, "https://github.example.com/acme/dotfiles/issues/3"])
    es = [{"name": "tooling", "rank": 1, "match": ["dotfiles"]}]
    hit = project_priority.of_task(t, {}, es)
    assert hit and hit["name"] == "tooling", "the repo comes from the second link"
