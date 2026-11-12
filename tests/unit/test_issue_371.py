"""Issue #371: synthetic proposal receipts and restricted inbox identifiers.

Each case owns its data and proof roots. The three receipt cases are after-only;
the baseline selects only the three primary and six preservation cases.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re
import socket
import subprocess
import sys
import webbrowser
from types import SimpleNamespace

import pytest

NOW = "2026-11-02T10:00:00-04:00"
DUE = "2026-11-03"
TITLE = "SYN-INBOX-TITLE"
WHY = "SYN-INBOX-REASON"
INVALID_REFS = ("t160x", "prefix-t160", "t１６０", "t160\nSYN-REF-CANARY", "SYN-REF-CANARY", "")


@pytest.fixture
def world(home, tmp_path, monkeypatch, capsys, request):
    from careerbrew import actions, cli, connectors, models, notify, plugins, secrets, store
    from careerbrew.day import inbox
    from careerbrew.work import priorities

    attempts = []

    def deny(*_args, **_kwargs):
        attempts.append("forbidden-host-call")
        raise AssertionError("issue371 attempted an undeclared host operation")

    for module, name in (
        (socket, "socket"),
        (socket, "getaddrinfo"),
        (subprocess, "Popen"),
        (connectors, "http_json"),
        (models, "complete"),
        (secrets, "resolve"),
        (notify, "deliver"),
        (notify, "remind"),
        (notify, "open_view"),
        (webbrowser, "open"),
    ):
        monkeypatch.setattr(module, name, deny)
    monkeypatch.setattr(sys.stdin, "isatty", lambda: False)
    monkeypatch.setattr(sys.stdout, "isatty", lambda: False)
    monkeypatch.delenv("CAREERBREW_TEST_OWNER", raising=False)
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    for key, folder in (("XDG_CONFIG_HOME", "xdg-config"), ("XDG_DATA_HOME", "xdg-data"), ("XDG_CACHE_HOME", "xdg-cache"), ("TMPDIR", "tmp")):
        path = tmp_path / folder
        path.mkdir()
        monkeypatch.setenv(key, str(path))
    evidence = tmp_path / "issue371-proof"
    evidence.mkdir()
    sequence = 0

    def fresh(name):
        root = tmp_path / name
        assert root.is_relative_to(tmp_path) and not root.exists()
        root.mkdir()
        monkeypatch.setenv("CAREERBREW_HOME", str(root))
        store.write_yaml(root / "profile.yaml", {})
        store.write_text(root / "sensitive-terms.txt", "")
        store.write_yaml(root / "work/connectors.yaml", {"connectors": []})
        assert not cli.may_show_business()
        return root

    def record(label, output=""):
        nonlocal sequence
        root = priorities.tasks_path().parents[1]
        assert root.is_relative_to(tmp_path)
        logical = {"tasks": priorities.load(), "inbox": inbox.load()}
        files = {str(p.relative_to(root)): {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "mode": p.stat().st_mode & 0o777} for p in root.rglob("*") if p.is_file()}
        assert logical == {"tasks": priorities.load(), "inbox": inbox.load()}
        assert files == {str(p.relative_to(root)): {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "mode": p.stat().st_mode & 0o777} for p in root.rglob("*") if p.is_file()}
        sequence += 1
        (evidence / f"{sequence:02d}-{label}.json").write_text(
            json.dumps({"case": request.node.name, "label": label, "now": NOW, "output": output, "logical": logical, "files": files, "host_attempts": attempts}, ensure_ascii=True, indent=2) + "\n"
        )
        return copy.deepcopy(logical)

    def run(*argv):
        capsys.readouterr()
        code = cli.main(list(argv))
        result = capsys.readouterr()
        record("cli", result.out + result.err)
        assert code == 0 and not result.err
        return result.out

    def install(name="syn-inbox", allow=False):
        source = tmp_path / f"source-{name}"
        source.mkdir()
        store.write_yaml(source / "careerbrew-plugin.yaml", {"name": name, "version": "1.0.0", "provides": {}, "policy": {"allowed_routes": ["local"], "agent_may_read_business": allow}})
        plugins.add(str(source))
        assert cli.may_show_business() is allow
        return name

    token = actions.OWNER_UI.set(False)
    try:
        fresh("initial-data")
        yield SimpleNamespace(cli=cli, store=store, inbox=inbox, priorities=priorities, plugins=plugins, run=run, fresh=fresh, record=record, install=install, monkeypatch=monkeypatch)
    finally:
        actions.OWNER_UI.reset(token)
        assert not attempts, attempts


def _add(w, title=TITLE, why=WHY):
    return w.priorities.add(title, 3, 3, 1, [], due=DUE, source="agent", why=why)


def _rows(w, opened=True, kind="agent-task"):
    return [r for r in w.inbox.load() if r["kind"] == kind and (not opened or r["status"] == "open")]


def _seed(w, done, proposed, shared=True):
    rows = []
    for n in range(1, done + proposed + 1):
        rows.append(
            {
                "id": f"t{n:03d}",
                "title": f"{TITLE}-{n}",
                "impact": 3,
                "urgency": 3,
                "estimate_h": 1,
                "competencies": [],
                "due": DUE,
                "status": "done" if n <= done else "proposed",
                "source": "agent",
                "added_at": NOW,
                "why": WHY if shared else f"{WHY}-{n}",
                "links": [],
            }
        )
    w.priorities.save(rows)
    for task in rows[done:]:
        w.inbox.offer_task(task)
    return rows


def _propose(w):
    return w.run("work", "task", TITLE, "--source", "agent", "--why", WHY, "--due", DUE)


def _group(w, kind="agent-task"):
    return next(g for g in w.inbox.view()["groups"] if g["kind"] == kind)


def _private(output):
    assert TITLE not in output and WHY not in output


@pytest.mark.parametrize("case", ["P01-receipt", "P02-t160-reference", "P03-101-members"])
def test_inbox_identity_primary(world, case):
    w = world
    if case == "P01-receipt":
        output = _propose(w)
        (task,), (row,) = w.priorities.load(), _rows(w)
        assert task["id"] == "t001" and task["due"] == DUE and task["added_at"] == NOW
        assert task["status"] == "proposed" and row["data"]["task"] == task["id"]
        _private(output)
        w.record("proposed-before-display-oracle", output)
        assert row["id"] in output and f"careerbrew day accept {row['id']}" in output
        w.inbox.accept(row["id"])
        state = w.record("explicit-synthetic-accept")
        assert [t["status"] for t in state["tasks"]] == ["open"]
        assert state["inbox"][0]["status"] == "accepted"
        return
    done, proposed = (157, 2) if case == "P02-t160-reference" else (59, 100)
    _seed(w, done, proposed)
    _propose(w)
    task = w.priorities.load()[-1]
    assert task["id"] == "t160" and task["status"] == "proposed" and task["due"] == DUE and task["added_at"] == NOW
    group = _group(w)
    assert group["count"] == proposed + 1
    before = w.record("group-before-cli")
    output = w.run("day", "inbox", "--group", group["id"])
    after = w.record("group-before-display-oracle", output)
    assert before["tasks"] == after["tasks"]
    rows = _rows(w)
    assert len(rows) == proposed + 1 and {r["id"] for r in rows} == set(group["ids"])
    _private(output)
    row = next(r for r in rows if r["data"]["task"] == "t160")
    if case == "P02-t160-reference":
        line = next(line for line in output.splitlines() if row["id"] in line)
        assert re.search(r"\bt160\b", line), output
    else:
        assert all(output.count(r["id"]) == 1 for r in rows), output
        for r in rows:
            line = next(line for line in output.splitlines() if r["id"] in line)
            assert re.search(rf"\b{r['data']['task']}\b", line)


@pytest.mark.parametrize("case", ["C01-owner", "C02-repeat", "C03-decisions", "C04-legacy-refs", "C05-owner-policy", "C06-groups-budget"])
def test_inbox_identity_guard(world, case):
    w = world
    if case == "C01-owner":
        with w.monkeypatch.context() as patch:
            patch.setenv("CAREERBREW_TEST_OWNER", "1")
            assert not w.cli.may_show_business()
            assert w.run("work", "task", TITLE, "--source", "user").strip() == "added t001"
        (task,) = w.priorities.load()
        assert task["status"] == "open" and task["source"] == "user" and not _rows(w)
    elif case == "C02-repeat":
        task = _add(w)
        w.inbox.offer_task(task)
        first = copy.deepcopy(_rows(w)[0])
        task.update(title=TITLE + "-UPDATED", why=WHY + "-UPDATED")
        w.priorities.save([task])
        w.inbox.offer_task(task)
        (row,) = _rows(w)
        assert (row["id"], row["created_at"]) == (first["id"], first["created_at"])
        assert row["data"]["title"] == task["title"] and row["data"]["why"] == task["why"]
        assert w.priorities.load()[0]["status"] == "proposed"
    elif case == "C03-decisions":
        _decisions(w, receipt=False)
    elif case == "C04-legacy-refs":
        refs = ("t160", *INVALID_REFS)
        tasks = [{"id": ref, "title": f"{TITLE}-{n}", "why": f"{WHY}-{n}", "source": "agent", "status": "proposed", "links": [{"label": f"SYN-LINK-{n}", "url": f"https://example.com/syn/{n}"}]} for n, ref in enumerate(refs)]
        w.priorities.save(tasks)
        for task in tasks:
            w.inbox.offer_task(task)
        output = w.run("day", "inbox")
        _private(output)
        assert "SYN-LINK" not in output and "example.com/syn" not in output
        assert all(ref not in output for ref in INVALID_REFS if ref)
        rows = _rows(w)
        assert len(rows) == len(refs)
        for row in rows:
            line = next(line for line in output.splitlines() if row["id"] in line)
            if row["data"]["task"] != "t160":
                assert not re.search(r"\bt160\b", line)
            for ref in INVALID_REFS:
                if ref:
                    assert ref not in line
    elif case == "C05-owner-policy":
        _seed(w, 0, 3)
        plugin = w.install(allow=True)
        output = w.run("day", "inbox", "--group", _group(w)["id"])
        for n in (1, 2, 3):
            assert f"{TITLE}-{n}" in output
        for text in (
            f"why: an agent added it: {WHY}",
            "from: an agent",
            "accept: Moves the task from proposed to open, so it shows in Week and the planner can schedule it.",
            "dismiss: Sets the task to dismissed; it is never planned.",
        ):
            assert output.count(text) == 3
        w.plugins.remove(plugin)
        assert not w.cli.may_show_business()
    else:
        _seed(w, 0, 3)
        other = _add(w, why=WHY + "-OTHER")
        w.inbox.offer_task(other)
        group = _group(w)
        view = w.inbox.view()
        assert group["count"] == 3
        assert len([r for r in view["inbox"] if r["kind"] == "agent-task" and not r.get("group")]) == 1
        w.record("agent-group-preserved")
        w.fresh("plugin-group-data")
        name = w.install()
        for n in range(3):
            w.inbox.suggest(name, f"SYN-PLUGIN-{n}", WHY, "Record this synthetic decision", ref=f"ref-{n}")
        plugin_group = _group(w, "plugin")
        assert plugin_group["count"] == 3 and len(set(plugin_group["ids"])) == 3
        output = w.run("day", "inbox", "--group", plugin_group["id"])
        assert all(output.count(iid) == 1 for iid in plugin_group["ids"])
        w.fresh("budget-data")
        _seed(w, 0, 101, shared=False)
        rows = _rows(w)
        output = w.run("day", "inbox")
        assert all(output.count(row["id"]) == 1 for row in rows[:100])
        assert rows[100]["id"] not in output and "more of" in output
    w.record("preservation-complete")


def _decisions(w, receipt):
    for action, status, task_status in (("accept", "accepted", "open"), ("dismiss", "dismissed", "dismissed")):
        w.fresh(f"decision-{action}")
        task = _add(w)
        w.inbox.offer_task(task)
        (row,) = _rows(w)
        getattr(w.inbox, action)(row["id"])
        before = w.record(f"{action}-decided")
        for _ in range(2):
            result = w.inbox.offer_task(task)
            if receipt:
                assert result == {"id": row["id"], "status": "decided"}
        after = w.record(f"{action}-reoffered")
        assert before == after and not _rows(w)
        assert after["tasks"][0]["status"] == task_status
        assert after["inbox"][0]["status"] == status and after["inbox"][0]["decided_at"] == NOW


@pytest.mark.parametrize("case", ["R01-new", "R02-updated", "R03-decided"])
def test_inbox_identity_receipt(world, case):
    w = world
    if case == "R03-decided":
        _decisions(w, receipt=True)
        return
    task = _add(w)
    result = w.inbox.offer_task(task)
    (row,) = _rows(w)
    if case == "R02-updated":
        created = row["created_at"]
        task.update(title=TITLE + "-UPDATED", why=WHY + "-UPDATED")
        w.priorities.save([task])
        result = w.inbox.offer_task(task)
        (updated,) = _rows(w)
        assert updated["created_at"] == created and updated["data"]["title"] == task["title"] and updated["data"]["why"] == task["why"]
    assert result == {"id": row["id"], "status": "new" if case == "R01-new" else "updated"}
    assert w.priorities.load()[0]["status"] == "proposed"
    w.record("after-only-receipt")
