#!/usr/bin/env python3
"""Print a repo-scoped review inventory for an approved work-day watch step."""

import argparse
import json
import os
import re
import selectors
import subprocess
import sys
import time
from typing import Any

REPO = re.compile(r"[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9_.-]+")
QUERY = """
query($owner: String!, $repo: String!, $cursor: String) {
  viewer { id }
  repository(owner: $owner, name: $repo) {
    nameWithOwner
    pullRequests(first: 50, after: $cursor, states: [OPEN], orderBy: {field: CREATED_AT, direction: ASC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        url number state author { ... on User { id } } isDraft mergeable headRefOid
        commits(last: 1) { nodes { commit { oid statusCheckRollup { state } } } }
        reviewThreads(first: 50) { pageInfo { hasNextPage } nodes { isResolved } }
        reviewRequests { totalCount }
        reviews(first: 50) { pageInfo { hasNextPage } nodes { state submittedAt } }
      }
    }
  }
}
"""


def complete_nodes(connection: dict[str, Any]) -> list[dict[str, Any]]:
    rows = connection["nodes"]
    if connection["pageInfo"]["hasNextPage"] is not False or not isinstance(rows, list):
        raise ValueError("incomplete GitHub page")
    return rows


def ready_row(pr: dict[str, Any], repo: str, viewer: str) -> dict[str, Any] | None:
    try:
        number = pr["number"]
        url = pr["url"]
        if (
            type(number) is not int
            or number < 1
            or url.casefold() != f"https://github.com/{repo}/pull/{number}".casefold()
            or pr["author"]["id"] != viewer
            or pr["state"] != "OPEN"
            or pr["isDraft"] is not False
            or pr["mergeable"] != "MERGEABLE"
        ):
            return None
        (last,) = pr["commits"]["nodes"]
        commit = last["commit"]
        if not pr["headRefOid"] or commit["oid"] != pr["headRefOid"] or commit["statusCheckRollup"]["state"] != "SUCCESS":
            return None
        if not all(thread["isResolved"] is True for thread in complete_nodes(pr["reviewThreads"])):
            return None
        requests = pr["reviewRequests"]["totalCount"]
        if type(requests) is not int or requests != 0:
            return None
        if any(review["state"] != "PENDING" or review["submittedAt"] is not None for review in complete_nodes(pr["reviews"])):
            return None
    except (AttributeError, KeyError, TypeError, ValueError):
        return None
    return {
        "url": url,
        "owned": True,
        "open": True,
        "draft": False,
        "checks_green": True,
        "mergeable": True,
        "threads_resolved": True,
        "reviewer_requested": False,
        "review_submitted": False,
    }


def inventory(response: dict[str, Any], repo: str) -> dict[str, Any]:
    if response.get("errors"):
        raise ValueError("GitHub query failed")
    data = response["data"]
    viewer = data["viewer"]["id"]
    repository = data["repository"]
    if not isinstance(viewer, str) or not viewer or repository["nameWithOwner"].casefold() != repo.casefold():
        raise ValueError("GitHub source mismatch")
    rows = complete_nodes(repository["pullRequests"])
    return {"complete": True, "pull_requests": [row for pr in rows if (row := ready_row(pr, repo, viewer)) is not None]}


MAX_PAGE_BYTES = 512 * 1024
MAX_OUTPUT_BYTES = 256 * 1024
MAX_PAGES = 20


def fetch_page(owner: str, repo: str, cursor: str | None, deadline: float) -> dict[str, Any]:
    argv = ["gh", "api", "--hostname", "github.com", "graphql", "-f", f"query={QUERY}", "-f", f"owner={owner}", "-f", f"repo={repo}"]
    if cursor is not None:
        argv += ["-f", f"cursor={cursor}"]
    process = subprocess.Popen(  # noqa: S603 (fixed read-only GitHub query)
        argv,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env={**os.environ, "GH_PROMPT_DISABLED": "1"},
    )
    try:
        assert process.stdout is not None
        data = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise subprocess.TimeoutExpired(argv, 30)
                chunk = os.read(process.stdout.fileno(), 8192)
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > MAX_PAGE_BYTES:
                    raise ValueError("page too large")
        if process.wait(timeout=max(0, deadline - time.monotonic())) != 0:
            raise ValueError("provider failed")
        return json.loads(data)
    finally:
        if process.poll() is None:
            process.kill()
        if process.stdout is not None:
            process.stdout.close()
        try:
            process.wait(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            pass


def collect_pages(owner: str, repo: str, deadline: float) -> dict[str, Any]:
    name = f"{owner}/{repo}"
    cursor = None
    cursors: set[str] = set()
    identities: set[int] = set()
    viewer = None
    ready: list[dict[str, Any]] = []
    for _ in range(MAX_PAGES):
        if time.monotonic() >= deadline:
            raise ValueError("deadline exceeded")
        response = fetch_page(owner, repo, cursor, deadline)
        if time.monotonic() >= deadline:
            raise ValueError("deadline exceeded")
        if response.get("errors"):
            raise ValueError("provider error")
        data = response["data"]
        current_viewer = data["viewer"]["id"]
        repository = data["repository"]
        if not isinstance(current_viewer, str) or not current_viewer or repository["nameWithOwner"].casefold() != name.casefold():
            raise ValueError("provider identity differs")
        if viewer is not None and viewer != current_viewer:
            raise ValueError("viewer changed")
        viewer = current_viewer
        connection = repository["pullRequests"]
        rows = connection["nodes"]
        if not isinstance(rows, list) or len(rows) > 50:
            raise ValueError("invalid page")
        for pr in rows:
            number, url = pr["number"], pr["url"]
            if type(number) is not int or number < 1 or number in identities or not isinstance(url, str) or url.casefold() != f"https://github.com/{name}/pull/{number}".casefold():
                raise ValueError("invalid or duplicate PR")
            identities.add(number)
            row = ready_row(pr, name, viewer)
            if row is not None:
                ready.append(row)
        info = connection["pageInfo"]
        more = info["hasNextPage"]
        if type(more) is not bool:
            raise ValueError("invalid page marker")
        if not more:
            end = info["endCursor"]
            if (rows and (not isinstance(end, str) or not end or len(end) > 4096 or end in cursors)) or (not rows and (end is not None or cursor is not None)):
                raise ValueError("invalid terminal cursor")
            result = {"complete": True, "pull_requests": ready}
            if len(json.dumps(result).encode()) + 1 > MAX_OUTPUT_BYTES:
                raise ValueError("inventory too large")
            if time.monotonic() >= deadline:
                raise ValueError("deadline exceeded")
            return result
        if not rows:
            raise ValueError("empty nonterminal page")
        cursor = info.get("endCursor")
        if not isinstance(cursor, str) or not cursor or len(cursor) > 4096 or cursor in cursors:
            raise ValueError("invalid page cursor")
        cursors.add(cursor)
    raise ValueError("page limit exceeded")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repo", metavar="OWNER/REPO")
    args = parser.parse_args()
    if not REPO.fullmatch(args.repo) or args.repo.split("/")[1] in {".", ".."}:
        parser.error("use an explicit OWNER/REPO")
    owner, repo = args.repo.split("/")
    try:
        result = collect_pages(owner, repo, time.monotonic() + 30)
        print(json.dumps(result))
    except (AttributeError, KeyError, OSError, RecursionError, TypeError, ValueError, subprocess.SubprocessError):
        print("GitHub review inventory unavailable or incomplete", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
