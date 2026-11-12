"""Collect evidence of work into `<data root>/work/evidence.jsonl`.

Sources (from `<data root>/work/sources.yaml` and plugin `evidence_sources`):
- git:  a local repo; commits by the configured author emails.
- gh:   pull requests via the GitHub CLI (`gh search prs --author @me`), with an
        optional `host` for GitHub Enterprise.
- note: manual entries (`careerbrew work note`), for things no tool records:
        a design review, mentoring, an incident you led.
- command: a plugin-declared command that prints JSON lines; runs only when
        the plugin was added with `--allow-exec`.

Each item has a stable id, so re-collecting never duplicates. An item seen
again with a newer kind or a corrected title replaces its stored copy (issue
#73). Git sources are cached by HEAD sha: an unchanged repo is not re-read.
All evidence is business data and is stored under the data root. Its exits
(a promo doc's model call, a message the owner sends) are in docs/privacy.md.
"""

from __future__ import annotations

import json
import shlex
import subprocess
from pathlib import Path
from typing import Any

from .. import events, paths, plugins, store
from ..day.results import VERSION_FIELDS

KIND_TAGS = {
    "fix": ["quality"],
    "feat": ["delivery"],
    "perf": ["quality", "impact"],
    "refactor": ["quality"],
    "docs": ["communication"],
    "test": ["quality"],
    "ci": ["tooling"],
    "chore": [],
}


def evidence_path() -> Path:
    return paths.sub("work", "evidence.jsonl")


def load() -> list[dict[str, Any]]:
    return store.read_jsonl(evidence_path())


def _existing_ids() -> set[str]:
    return {e["id"] for e in load()}


def tags_for(title: str) -> list[str]:
    low = title.lower()
    head = low.split(":", 1)[0].split("(", 1)[0].strip()
    tags = list(KIND_TAGS.get(head, []))
    for word, tag in (("incident", "ownership"), ("mentor", "leadership"), ("design", "design"), ("rfc", "design"), ("migrat", "impact"), ("latency", "impact"), ("review", "collaboration")):
        if word in low:
            tags.append(tag)
    return sorted(set(tags))


def _git(repo: Path, *args: str) -> str:
    res = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, check=False)
    if res.returncode != 0:
        raise RuntimeError(f"git failed in a configured repo (fix: check the path in work/sources.yaml): {res.stderr.strip()[:160]}")
    return res.stdout


def collect_git(src: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    repo = Path(src["path"]).expanduser()
    authors = src.get("authors") or []
    head = _git(repo, "rev-parse", "HEAD").strip()

    def read() -> list[dict[str, Any]]:
        fmt = "%H%x1f%aI%x1f%ae%x1f%s"
        out = _git(repo, "log", "--no-merges", f"--pretty=format:{fmt}", "--numstat")
        items: list[dict[str, Any]] = []
        cur: dict[str, Any] | None = None
        for line in out.splitlines():
            if "\x1f" in line:
                sha, when, email, subject = line.split("\x1f", 3)
                cur = None
                if authors and email not in authors:
                    continue
                cur = {"id": f"git:{repo.name}:{sha[:12]}", "kind": "commit", "repo": repo.name, "title": subject, "at": when, "added": 0, "deleted": 0, "files": 0}
                items.append(cur)
            elif cur is not None and line.strip():
                parts = line.split("\t")
                if len(parts) == 3:
                    cur["added"] += int(parts[0]) if parts[0].isdigit() else 0
                    cur["deleted"] += int(parts[1]) if parts[1].isdigit() else 0
                    cur["files"] += 1
        return items

    items, _hit = store.cached(["git-evidence", str(repo), head, authors], read)
    for it in items:
        it["tags"] = tags_for(it["title"])
        it["origin"] = origin
    return items


def collect_gh(src: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    cmd = ["gh", "search", "prs", "--author", src.get("author", "@me"), "--limit", str(src.get("limit", 200)), "--json", "number,title,url,createdAt,closedAt,state,repository,isDraft"]
    env = None
    if src.get("host"):
        import os

        env = {**os.environ, "GH_HOST": src["host"]}
    res = subprocess.run(cmd, capture_output=True, text=True, check=False, env=env)
    if res.returncode != 0:
        raise RuntimeError(f"gh failed (fix: run `gh auth status`): {res.stderr.strip()[:160]}")
    items = []
    for pr in json.loads(res.stdout or "[]"):
        repo = (pr.get("repository") or {}).get("nameWithOwner", "")
        items.append(
            {
                "id": f"pr:{repo}#{pr['number']}",
                "kind": "pr",
                "repo": repo,
                "title": pr["title"],
                "url": pr["url"],
                "at": pr["createdAt"],
                "state": pr.get("state"),
                "draft": pr.get("isDraft") is True,
                "tags": tags_for(pr["title"]),
                "origin": origin,
            }
        )
    return items


def collect_command(src: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    name = origin.split(":", 1)[-1]
    entry = next((p for p in plugins.installed() if p["name"] == name), None)
    if not entry or not entry.get("exec_allowed"):
        raise PermissionError(f"command sources from {origin} need `careerbrew plugin add <src> --allow-exec`")
    res = subprocess.run(shlex.split(src["run"]), capture_output=True, text=True, check=False, cwd=entry["root"], timeout=120)
    if res.returncode != 0:
        raise RuntimeError(f"plugin command failed: exit {res.returncode}")
    items = []
    for line in res.stdout.splitlines():
        if line.strip():
            it = json.loads(line)
            it.setdefault("kind", "external")
            it["id"] = f"{name}:{it['id']}"
            it["tags"] = sorted(set(it.get("tags", [])) | set(tags_for(it.get("title", ""))))
            it["origin"] = origin
            items.append(it)
    return items


def sources_list() -> list[dict[str, Any]]:
    out = [{**s, "origin": "user"} for s in (store.read_yaml(paths.data_root() / "work" / "sources.yaml", {}) or {}).get("sources", [])]
    out.extend({**c, "origin": "user"} for c in (store.read_yaml(paths.data_root() / "work" / "connectors.yaml", {}) or {}).get("connectors", []))
    for origin, path in plugins.resources("evidence_sources"):
        data = store.read_yaml(path, {}) or {}
        out.extend({**s, "origin": origin} for s in data.get("sources", []) + data.get("connectors", []))
    return out


def collect() -> dict[str, Any]:
    have = _existing_ids()
    found: list[dict[str, Any]] = []
    errors = 0
    reasons: list[str] = []
    for src in sources_list():
        from .. import connectors  # local import: connectors imports this module

        fn = {"git": collect_git, "gh": collect_gh, "command": collect_command}.get(src["kind"])
        if fn is None and src["kind"] in connectors.KINDS:
            fn = connectors.run
        if fn is None:
            continue
        try:
            found.extend(it for it in fn(src, src["origin"]) if not it.get("personal"))  # personal events never become evidence (issue #72)
        except (RuntimeError, PermissionError, OSError, KeyError, ValueError, subprocess.TimeoutExpired) as exc:
            errors += 1
            # Our own error messages name the fix and never echo a secret value.
            from .. import privacy

            reasons.append(f"{src['kind']}: {privacy.redact(str(exc))[:200]}")
            events.emit("work.collect.error", source_kind=src["kind"], story="us-08")
    from ..calendar import now_utc  # local: calendar reaches this module through connectors
    from . import feed  # local import: feed reads this module's sources

    found.extend(feed.evidence_items())  # your own actions from the work feed, such as a page you resolved
    at = now_utc().isoformat(timespec="seconds")
    # A known id with a newer kind or a corrected title replaces its stored row (issues #52, #73); `collected_at`
    # is written only with a row that is new or replaced.
    new, _replaced = store.upsert_jsonl(evidence_path(), [{**it, "collected_at": at} for it in found], VERSION_FIELDS)
    events.emit("work.collect", status="ok", new=new, errors=errors)
    return {"new": new, "total": len(have) + new, "errors": errors, "reasons": reasons}


def forget(ids: set[str]) -> int:
    """Remove evidence rows by id (an event the owner marked personal). Returns how many went."""
    rows = load()
    keep = [e for e in rows if e.get("id") not in ids]
    if len(keep) != len(rows):
        store.write_jsonl(evidence_path(), keep)
    return len(rows) - len(keep)


def note(title: str, tags: list[str], impact: str = "", at: str | None = None, links: list[str] | None = None) -> dict[str, Any]:
    from ..calendar import now as clock_now

    when = at or clock_now().isoformat(timespec="seconds")  # same clock as day plans and results
    item = {"id": "note:" + store.digest(title, when), "kind": "note", "title": title, "impact": impact, "at": when, "tags": sorted(set(tags) | set(tags_for(title))), "origin": "user"}
    if links:
        item["links"] = list(links)
    store.append_jsonl(evidence_path(), [item])
    events.emit("work.note", status="ok")
    return item
