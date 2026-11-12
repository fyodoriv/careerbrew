"""GitHub inventory only offers PRs with complete provider evidence."""

import json
import shlex
import sys
from pathlib import Path
from runpy import run_path

import pytest
import yaml

from careerbrew import cli, plugins
from careerbrew.work import review_ask
from careerbrew.workday import questions, watch

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def adapter():
    return run_path(str(ROOT / "scripts/review-github-inventory.py"))["inventory"]


@pytest.fixture
def response():
    def page(*nodes):
        return {"pageInfo": {"hasNextPage": False, "endCursor": "fixture-end" if nodes else None}, "nodes": list(nodes)}

    pr = {
        "url": "https://github.com/example/review-demo/pull/41",
        "number": 41,
        "state": "OPEN",
        "author": {"id": "owner-node"},
        "isDraft": False,
        "mergeable": "MERGEABLE",
        "headRefOid": "a" * 40,
        "commits": {"nodes": [{"commit": {"oid": "a" * 40, "statusCheckRollup": {"state": "SUCCESS"}}}]},
        "reviewThreads": page({"isResolved": True}),
        "reviewRequests": {"totalCount": 0},
        "reviews": page(),
    }
    return {"data": {"viewer": {"id": "owner-node"}, "repository": {"nameWithOwner": "Example/review-demo", "pullRequests": page(pr)}}}, pr


def test_complete_ready_pr(adapter, response):
    raw, pr = response
    assert adapter(raw, "example/review-demo") == {
        "complete": True,
        "pull_requests": [
            {
                "url": pr["url"],
                "owned": True,
                "open": True,
                "draft": False,
                "checks_green": True,
                "mergeable": True,
                "threads_resolved": True,
                "reviewer_requested": False,
                "review_submitted": False,
            }
        ],
    }


@pytest.mark.parametrize(
    "field,value",
    [
        ("headRefOid", "b" * 40),
        ("commits", {"nodes": [{"commit": {"oid": "a" * 40, "statusCheckRollup": None}}]}),
        ("author", {"id": "other-node"}),
        ("reviewThreads", {"pageInfo": {"hasNextPage": True}, "nodes": []}),
        ("reviewThreads", {"pageInfo": {"hasNextPage": False}, "nodes": [{"isResolved": False}]}),
        ("reviewRequests", {"totalCount": 1}),
        ("reviews", {"pageInfo": {"hasNextPage": True}, "nodes": []}),
        ("reviews", {"pageInfo": {"hasNextPage": False}, "nodes": [{"state": "COMMENTED", "submittedAt": "2026-11-04T12:00:00Z"}]}),
    ],
)
def test_missing_stale_or_ineligible_evidence_drops_row(adapter, response, field, value):
    raw, pr = response
    pr[field] = value
    assert adapter(raw, "example/review-demo") == {"complete": True, "pull_requests": []}


def test_truncated_repository_inventory_is_rejected(adapter, response):
    raw, _ = response
    raw["data"]["repository"]["pullRequests"]["pageInfo"]["hasNextPage"] = True
    with pytest.raises(ValueError):
        adapter(raw, "example/review-demo")


def test_watch_uses_repo_scoped_github_adapter(response, tmp_path, home, monkeypatch):
    raw, pr = response
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-04T15:00+00:00")
    monkeypatch.setattr(review_ask, "recheck_pr", lambda _: ("unknown", "fixture"))
    cli.main(["init"])

    command_dir = tmp_path / "bin"
    command_dir.mkdir()
    fixture = tmp_path / "provider.json"
    gh = command_dir / "gh"
    gh.write_text(f'#!/bin/sh\n[ "$1 $2 $3 $4" = "api --hostname github.com graphql" ] || exit 2\n[ "$8" = owner=example ] && [ "${{10}}" = repo=review-demo ] || exit 2\ncat {shlex.quote(str(fixture))}\n')
    gh.chmod(0o700)
    monkeypatch.setenv("PATH", f"{command_dir}:/usr/bin:/bin")
    plugin = tmp_path / "review-plugin"
    plugin.mkdir()
    command = shlex.join([sys.executable, str(ROOT / "scripts/review-github-inventory.py"), "example/review-demo"])
    (plugin / plugins.MANIFEST).write_text(
        yaml.safe_dump(
            {
                "name": "review-plugin",
                "version": "1.0.0",
                "work_steps": [{"step": "/review-prs", "review_ask_cycle": command}],
            }
        )
    )
    plugins.add(str(plugin), allow_exec=True)

    checks = pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]
    checks["state"] = "PENDING"
    fixture.write_text(json.dumps(raw))
    first = watch.run_pass()
    assert any(row["step"] == "/review-prs" and row["result"] == "ok" for row in first["steps"])
    assert questions.open_records() == []

    checks["state"] = "SUCCESS"
    fixture.write_text(json.dumps(raw))
    watch.run_pass()
    (offer,) = questions.open_records()
    assert offer["recheck"] == f"careerbrew:review-pr {pr['url']}"
    watch.run_pass()
    assert [row["id"] for row in questions.open_records()] == [offer["id"]]


def test_inventory_traverses_two_pages_without_losing_ready_pr(response, monkeypatch, capsys, tmp_path):
    import copy

    raw, ready = response
    ready["number"] = 51
    ready["url"] = "https://github.com/example/review-demo/pull/51"
    first = copy.deepcopy(raw)
    rows = []
    for number in range(1, 51):
        row = copy.deepcopy(ready)
        row["number"] = number
        row["url"] = f"https://github.com/example/review-demo/pull/{number}"
        row["author"] = {"id": "other-node"}
        rows.append(row)
    connection = first["data"]["repository"]["pullRequests"]
    connection["nodes"] = rows
    connection["pageInfo"] = {"hasNextPage": True, "endCursor": "synthetic-cursor"}
    second = copy.deepcopy(raw)
    second["data"]["repository"]["pullRequests"]["pageInfo"]["endCursor"] = "terminal-cursor"
    first_file = tmp_path / "first.json"
    second_file = tmp_path / "second.json"
    first_file.write_text(json.dumps(first))
    second_file.write_text(json.dumps(second))
    gh = tmp_path / "gh"
    gh.write_text(f'#!/bin/sh\ncase "$*" in *cursor=synthetic-cursor*) cat {shlex.quote(str(second_file))};; *) cat {shlex.quote(str(first_file))};; esac\n')
    gh.chmod(0o700)
    monkeypatch.setenv("PATH", f"{tmp_path}:/usr/bin:/bin")
    monkeypatch.setattr(sys, "argv", ["inventory", "example/review-demo"])
    main = run_path(str(ROOT / "scripts/review-github-inventory.py"))["main"]
    assert main() == 0
    result = json.loads(capsys.readouterr().out)
    assert result["complete"] is True
    assert [row["url"] for row in result["pull_requests"]] == [ready["url"]]


@pytest.mark.parametrize("failure", ["missing_cursor", "repeated_cursor", "invalid_marker", "duplicate", "viewer_changed", "repo_changed", "later_error"])
def test_paged_inventory_refuses_incomplete_or_inconsistent_pages(response, monkeypatch, failure):
    import copy
    import time

    raw, pr = response
    first = copy.deepcopy(raw)
    connection = first["data"]["repository"]["pullRequests"]
    connection["pageInfo"] = {"hasNextPage": True, "endCursor": "cursor-one"}
    second = copy.deepcopy(raw)
    second_row = second["data"]["repository"]["pullRequests"]["nodes"][0]
    second_row["number"] = 42
    second_row["url"] = "https://github.com/example/review-demo/pull/42"
    if failure == "missing_cursor":
        connection["pageInfo"].pop("endCursor")
    elif failure == "repeated_cursor":
        second["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": "cursor-one"}
    elif failure == "invalid_marker":
        connection["pageInfo"]["hasNextPage"] = "yes"
    elif failure == "duplicate":
        second["data"]["repository"]["pullRequests"]["nodes"] = [copy.deepcopy(pr)]
    elif failure == "viewer_changed":
        second["data"]["viewer"]["id"] = "another-viewer"
    elif failure == "repo_changed":
        second["data"]["repository"]["nameWithOwner"] = "example/another"
    elif failure == "later_error":
        second = {"errors": [{"message": "synthetic"}]}
    pages = iter((first, second))
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: next(pages))
    with pytest.raises((ValueError, KeyError)):
        collect("example", "review-demo", time.monotonic() + 5)


def test_inventory_request_stops_at_deadline(tmp_path, monkeypatch):
    import subprocess
    import time

    gh = tmp_path / "gh"
    gh.write_text("#!/bin/sh\nexec /bin/sleep 3\n")
    gh.chmod(0o700)
    monkeypatch.setenv("PATH", f"{tmp_path}:/usr/bin:/bin")
    fetch = run_path(str(ROOT / "scripts/review-github-inventory.py"))["fetch_page"]
    start = time.monotonic()
    with pytest.raises(subprocess.TimeoutExpired):
        fetch("example", "review-demo", None, start + 0.2)
    assert time.monotonic() - start < 1.5


def test_later_page_failure_prints_no_partial_inventory(response, monkeypatch, capsys):
    import copy

    raw, _ = response
    first = copy.deepcopy(raw)
    first["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": "cursor-one"}
    pages = iter((first, {"errors": [{"message": "synthetic private detail"}]}))
    main = run_path(str(ROOT / "scripts/review-github-inventory.py"))["main"]
    monkeypatch.setitem(main.__globals__, "fetch_page", lambda *_: next(pages))
    monkeypatch.setattr(sys, "argv", ["inventory", "example/review-demo"])
    assert main() == 1
    out = capsys.readouterr()
    assert out.out == ""
    assert "synthetic private detail" not in out.err


def test_inventory_rejects_oversized_page(tmp_path, monkeypatch):
    import time

    gh = tmp_path / "gh"
    gh.write_text(f"#!/bin/sh\nexec {shlex.quote(sys.executable)} -c 'import sys; sys.stdout.write(\"x\" * 600000)'\n")
    gh.chmod(0o700)
    monkeypatch.setenv("PATH", f"{tmp_path}:/usr/bin:/bin")
    fetch = run_path(str(ROOT / "scripts/review-github-inventory.py"))["fetch_page"]
    with pytest.raises(ValueError, match="page too large"):
        fetch("example", "review-demo", None, time.monotonic() + 5)


def test_paged_inventory_respects_page_limit(response, monkeypatch):
    import copy
    import time

    raw, _ = response
    counter = iter(range(1, 4))

    def page(*_):
        number = next(counter)
        value = copy.deepcopy(raw)
        pr = value["data"]["repository"]["pullRequests"]["nodes"][0]
        pr["number"] = number
        pr["url"] = f"https://github.com/example/review-demo/pull/{number}"
        value["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": f"cursor-{number}"}
        return value

    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    monkeypatch.setitem(collect.__globals__, "fetch_page", page)
    monkeypatch.setitem(collect.__globals__, "MAX_PAGES", 2)
    with pytest.raises(ValueError, match="page limit"):
        collect("example", "review-demo", time.monotonic() + 5)


def test_nonterminal_empty_page_and_bad_terminal_cursor_are_refused(response, monkeypatch):
    import copy
    import time

    raw, _ = response
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    for nodes, info in [([], {"hasNextPage": True, "endCursor": "next"}), ([raw["data"]["repository"]["pullRequests"]["nodes"][0]], {"hasNextPage": False, "endCursor": None})]:
        page = copy.deepcopy(raw)
        page["data"]["repository"]["pullRequests"] = {"nodes": nodes, "pageInfo": info}
        monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: page)
        with pytest.raises(ValueError):
            collect("example", "review-demo", time.monotonic() + 5)


def test_shared_deadline_refuses_late_final_page(response, monkeypatch):
    import copy
    import time

    raw, _ = response
    first = copy.deepcopy(raw)
    first["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": "next"}
    second = copy.deepcopy(raw)
    row = second["data"]["repository"]["pullRequests"]["nodes"][0]
    row["number"] = 42
    row["url"] = "https://github.com/example/review-demo/pull/42"
    pages = iter((first, second))

    def slow(*_):
        time.sleep(0.08)
        return next(pages)

    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    monkeypatch.setitem(collect.__globals__, "fetch_page", slow)
    with pytest.raises(ValueError, match="deadline"):
        collect("example", "review-demo", time.monotonic() + 0.12)


def test_deep_json_failure_does_not_print_traceback(tmp_path, monkeypatch, capsys):
    gh = tmp_path / "gh"
    gh.write_text("#!/bin/sh\nprintf '%01000d' 0 | tr 0 '['\n")
    gh.chmod(0o700)
    monkeypatch.setenv("PATH", f"{tmp_path}:/usr/bin:/bin")
    monkeypatch.setattr(sys, "argv", ["inventory", "example/review-demo"])
    main = run_path(str(ROOT / "scripts/review-github-inventory.py"))["main"]
    assert main() == 1
    result = capsys.readouterr()
    assert result.out == ""
    assert result.err == "GitHub review inventory unavailable or incomplete\n"


def test_collect_pages_preserves_nested_evidence_checks(response, monkeypatch):
    import copy
    import time

    raw, _ = response
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    for field, value in [("headRefOid", "b" * 40), ("reviewThreads", {"pageInfo": {"hasNextPage": True}, "nodes": []})]:
        page = copy.deepcopy(raw)
        page["data"]["repository"]["pullRequests"]["nodes"][0][field] = value
        monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: page)
        assert collect("example", "review-demo", time.monotonic() + 5) == {"complete": True, "pull_requests": []}


def test_inventory_output_limit_counts_newline(response, monkeypatch):
    import time

    raw, _ = response
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: raw)
    result = collect("example", "review-demo", time.monotonic() + 5)
    exact_without_newline = len(json.dumps(result).encode())
    monkeypatch.setitem(collect.__globals__, "MAX_OUTPUT_BYTES", exact_without_newline)
    with pytest.raises(ValueError, match="inventory too large"):
        collect("example", "review-demo", time.monotonic() + 5)


def test_later_page_timeout_prints_no_partial_inventory(response, monkeypatch, capsys):
    import copy
    import subprocess

    raw, _ = response
    first = copy.deepcopy(raw)
    first["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": "cursor-one"}
    calls = iter((first, None))

    def fetch(*_):
        page = next(calls)
        if page is None:
            raise subprocess.TimeoutExpired(["synthetic-private-argument"], 30)
        return page

    main = run_path(str(ROOT / "scripts/review-github-inventory.py"))["main"]
    monkeypatch.setitem(main.__globals__, "fetch_page", fetch)
    monkeypatch.setattr(sys, "argv", ["inventory", "example/review-demo"])
    assert main() == 1
    out = capsys.readouterr()
    assert out.out == ""
    assert "synthetic-private-argument" not in out.err


def test_terminal_page_refuses_repeated_or_missing_cursor(response, monkeypatch):
    import copy
    import time

    raw, _ = response
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    for terminal in ({"hasNextPage": False, "endCursor": "next"}, {"hasNextPage": False}):
        first = copy.deepcopy(raw)
        first["data"]["repository"]["pullRequests"]["pageInfo"] = {"hasNextPage": True, "endCursor": "next"}
        second = copy.deepcopy(raw)
        row = second["data"]["repository"]["pullRequests"]["nodes"][0]
        row["number"] = 42
        row["url"] = "https://github.com/example/review-demo/pull/42"
        second["data"]["repository"]["pullRequests"]["pageInfo"] = terminal
        pages = iter((first, second))
        monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: next(pages))
        with pytest.raises((ValueError, KeyError)):
            collect("example", "review-demo", time.monotonic() + 5)


def test_inventory_checks_deadline_after_validation(response, monkeypatch):
    from types import SimpleNamespace

    raw, _ = response
    collect = run_path(str(ROOT / "scripts/review-github-inventory.py"))["collect_pages"]
    clock = iter((0.0, 0.1, 2.0))
    monkeypatch.setitem(collect.__globals__, "time", SimpleNamespace(monotonic=lambda: next(clock)))
    monkeypatch.setitem(collect.__globals__, "fetch_page", lambda *_: raw)
    with pytest.raises(ValueError, match="deadline"):
        collect("example", "review-demo", 1.0)
