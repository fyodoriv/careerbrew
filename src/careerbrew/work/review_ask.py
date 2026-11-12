"""Review asks that name enough people for the PR's required approvals (issue #270, US-90).

A review ask that names one person, or only a group handle such as `@org/team`, stalls a PR that
needs two approvals. `build(url)` reads, through the GitHub connector:

- the required approvals for the PR's base branch: branch protection and the branch's rulesets, the
  higher one wins; a read that is refused (403) or absent (404) counts as 0;
- the changed files, the repo's CODEOWNERS (`.github/`, the root, then `docs/`), and the reviews of the
  last `RECENT_PRS` closed PRs on the same base.

It names at least `max(required, MIN_PEOPLE)` people: code owners of the changed paths first, then
recent reviewers of those paths, then people the PR already asks. Group handles never count. Each
person gets the paths to review. `check(text, needed)` refuses a draft that names too few people.

Privacy: logins, paths, and the title are business data. The CLI prints counts to an agent and the
draft only where business data may show (`may_show_business`); nothing is stored. Events carry counts.
"""

from __future__ import annotations

import base64
import re
import urllib.error
import urllib.parse
from collections import Counter
from pathlib import PurePosixPath
from typing import Any

from .. import connectors, events, secrets
from . import evidence

MIN_PEOPLE = 2  # never fewer than two, whatever the branch rules say
RECENT_PRS = 20  # closed PRs read for recent reviewers
SHOW_PATHS = 4  # paths named per person before "and N more"
CODEOWNERS = (".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS")  # GitHub's own lookup order
PR_URL = re.compile(r"^https?://(?P<host>[^/]+)/(?P<owner>[\w.-]+)/(?P<repo>[\w.-]+)/pull/(?P<number>\d+)")
CHAT_WIDE = {"here", "channel", "everyone", "all"}  # chat mentions, never a reviewer
MENTION = re.compile(r"(?<![\w@])@([A-Za-z0-9][A-Za-z0-9-]*(?:/[A-Za-z0-9][\w.-]*)?)")


class ReviewAskError(RuntimeError):
    """The draft cannot be built or is refused; the message names the fix."""


def parse_url(url: str) -> dict[str, Any]:
    m = PR_URL.match(url.strip())
    if not m:
        raise ReviewAskError(f"not a pull request link: {url[:80]} (fix: pass https://<host>/<owner>/<repo>/pull/<number>)")
    return {**m.groupdict(), "number": int(m["number"])}


def _connector() -> dict[str, Any]:
    allowed = connectors.plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and "github" not in allowed:
        raise ReviewAskError("github is not in the plugin policy allowed_connectors")
    for c in evidence.sources_list():
        if c.get("kind") == "github":
            return c
    raise ReviewAskError("no github connector (fix: careerbrew connect github)")


def _web_host(api: str) -> str:
    """The web host whose PR links belong to an API root: api.github.com is github.com, <host>/api/v3 is <host>."""
    host = urllib.parse.urlparse(api).hostname or ""
    return host.removeprefix("api.")


def _open(url: str) -> tuple[dict[str, Any], str, dict[str, str], dict[str, Any]]:
    """The parsed link, the repo's API root, the auth header, and the PR."""
    ref = parse_url(url)
    c = _connector()
    api = str(c.get("api") or "https://api.github.com").rstrip("/")
    if ref["host"].lower() != _web_host(api):
        # The token goes only to the connector's own API: a link to another host would send it there.
        raise ReviewAskError(f"the link's host is not the github connector's host ({_web_host(api)}) (fix: pass a pull request link on {_web_host(api)})")
    hdr = {"Authorization": f"Bearer {secrets.resolve(c['token'])}"}
    repo_api = f"{api}/repos/{ref['owner']}/{ref['repo']}"
    return ref, repo_api, hdr, connectors.http_json(f"{repo_api}/pulls/{ref['number']}", hdr)


def _get(url: str, hdr: dict[str, str], missing: Any = None) -> Any:
    """A GET whose 403 or 404 means "not there for this token": returns `missing`."""
    try:
        return connectors.http_json(url, hdr)
    except urllib.error.HTTPError as e:
        if e.code in (403, 404):
            return missing
        raise


def is_group(handle: str) -> bool:
    return "/" in handle.lstrip("@")


def required_approvals(repo_api: str, base: str, hdr: dict[str, str]) -> dict[str, Any]:
    """The approvals the base branch requires: {count, source}; source says which rule set it, or `unknown`."""
    b = urllib.parse.quote(base, safe="")
    found: list[tuple[int, str]] = []
    prot = _get(f"{repo_api}/branches/{b}/protection", hdr)
    if isinstance(prot, dict):
        found.append((int((prot.get("required_pull_request_reviews") or {}).get("required_approving_review_count") or 0), "branch protection"))
    rules = _get(f"{repo_api}/rules/branches/{b}", hdr)
    if isinstance(rules, list):
        counts = [int((r.get("parameters") or {}).get("required_approving_review_count") or 0) for r in rules if r.get("type") == "pull_request"]
        found.append((max(counts, default=0), "rulesets"))
    if not found:
        return {"count": 0, "source": "unknown"}
    count, source = max(found)
    return {"count": count, "source": source}


def _pattern(pat: str) -> re.Pattern[str]:
    """A CODEOWNERS pattern (gitignore rules) as a regex over a repo path."""
    anchored = pat.startswith("/") or "/" in pat.rstrip("/")
    body = pat.strip("/")
    out = ""
    i = 0
    while i < len(body):
        if body.startswith("**/", i):
            out += "(?:.*/)?"
            i += 3
        elif body.startswith("**", i):
            out += ".*"
            i += 2
        elif body[i] == "*":
            out += "[^/]*"
            i += 1
        elif body[i] == "?":
            out += "[^/]"
            i += 1
        else:
            out += re.escape(body[i])
            i += 1
    prefix = "" if anchored else "(?:.*/)?"
    below = "" if pat.endswith("/*") else "(?:/.*)?"  # GitHub: `docs/*` owns docs/a.md, not docs/sub/a.md
    return re.compile(f"^{prefix}{out}{below}$")


def parse_codeowners(text: str) -> list[tuple[re.Pattern[str], list[str]]]:
    rules = []
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        pat, *owners = line.split()
        rules.append((_pattern(pat), [o.lstrip("@") for o in owners if o.startswith("@")]))  # an email owner cannot be @-named
    return rules


def owners_of(path: str, rules: list[tuple[re.Pattern[str], list[str]]]) -> list[str]:
    """The last matching rule wins, as on GitHub."""
    hit: list[str] = []
    for rx, owners in rules:
        if rx.match(path):
            hit = owners
    return hit


def _codeowners(repo_api: str, base: str, hdr: dict[str, str]) -> list[tuple[re.Pattern[str], list[str]]]:
    for where in CODEOWNERS:
        got = _get(f"{repo_api}/contents/{where}?ref={urllib.parse.quote(base, safe='')}", hdr)
        if isinstance(got, dict) and got.get("content"):
            return parse_codeowners(base64.b64decode(got["content"]).decode("utf-8", errors="replace"))
    return []


def _near(a: str, b: str) -> bool:
    """Same file, or files in the same folder."""
    return a == b or PurePosixPath(a).parent == PurePosixPath(b).parent


def _bot(user: dict[str, Any]) -> bool:
    return user.get("type") == "Bot" or str(user.get("login", "")).endswith("[bot]")


def _recent_reviewers(repo_api: str, base: str, number: int, files: list[str], hdr: dict[str, str]) -> dict[str, list[str]]:
    """login -> the changed paths near files that person reviewed in recent closed PRs, most active first."""
    pulls = _get(f"{repo_api}/pulls?state=closed&base={urllib.parse.quote(base, safe='')}&sort=updated&direction=desc&per_page={RECENT_PRS}", hdr, []) or []
    seen: Counter[str] = Counter()
    paths: dict[str, set[str]] = {}
    for pr in pulls[:RECENT_PRS]:
        if pr.get("number") == number:
            continue
        theirs = [f.get("filename", "") for f in _get(f"{repo_api}/pulls/{pr['number']}/files?per_page=100", hdr, []) or []]
        mine = {f for f in files if any(_near(f, t) for t in theirs)}
        if not mine:
            continue
        author = (pr.get("user") or {}).get("login")
        for login in {(r.get("user") or {}).get("login") for r in _get(f"{repo_api}/pulls/{pr['number']}/reviews?per_page=100", hdr, []) or [] if not _bot(r.get("user") or {})}:
            if login and login != author:
                seen[login] += 1
                paths.setdefault(login, set()).update(mine)
    return {login: sorted(paths[login]) for login, _n in seen.most_common()}


def pick(needed: int, author: str, code_owners: dict[str, list[str]], recent: dict[str, list[str]], requested: list[str], extra: list[str]) -> list[dict[str, Any]]:
    """Up to `needed` people (more when the owner adds them with --reviewer), each with why and what to review."""
    out: list[dict[str, Any]] = []
    taken = {author.lower()}

    def take(login: str, why: str, paths: list[str]) -> None:
        if login and not is_group(login) and login.lower() not in taken:
            taken.add(login.lower())
            out.append({"login": login, "why": why, "paths": paths})

    for login in extra:
        take(login.lstrip("@"), "you named them", [])
    for login, paths in code_owners.items():
        if len(out) < needed:
            take(login, "code owner", paths)
    for login, paths in recent.items():
        if len(out) < needed:
            take(login, "reviewed recent changes here", paths)
    for login in requested:
        if len(out) < needed:
            take(login, "already asked on the PR", [])
    return out


def _what(paths: list[str], all_files: list[str]) -> str:
    if not paths or set(paths) >= set(all_files):
        return "the whole change"
    shown = ", ".join(paths[:SHOW_PATHS])
    return shown + (f" and {len(paths) - SHOW_PATHS} more" if len(paths) > SHOW_PATHS else "")


def build(url: str, extra: list[str] | None = None) -> dict[str, Any]:
    """The draft and its counts. `status` is `ready`, or `short` when too few people could be found."""
    ref, repo_api, hdr, pr = _open(url)
    base = (pr.get("base") or {}).get("ref") or "main"
    author = (pr.get("user") or {}).get("login", "")
    files = [f.get("filename", "") for f in connectors.http_json(f"{repo_api}/pulls/{ref['number']}/files?per_page=100", hdr) or []]
    req = required_approvals(repo_api, base, hdr)
    needed = max(req["count"], MIN_PEOPLE)
    rules = _codeowners(repo_api, base, hdr)
    owned: dict[str, list[str]] = {}
    groups: set[str] = set()
    for f in files:
        for o in owners_of(f, rules):
            if is_group(o):
                groups.add(o)
            else:
                owned.setdefault(o, []).append(f)
    owned = dict(sorted(owned.items(), key=lambda kv: -len(kv[1])))  # who owns most of the change first
    recent = _recent_reviewers(repo_api, base, ref["number"], files, hdr)
    requested = [u.get("login", "") for u in pr.get("requested_reviewers") or [] if not _bot(u)]
    groups |= {f"{ref['owner']}/{t.get('slug', '')}" for t in pr.get("requested_teams") or []}
    people = pick(needed, author, owned, recent, requested, extra or [])
    status = "ready" if len(people) >= needed else "short"
    why_count = f"{req['count']} required by {req['source']}" if req["source"] != "unknown" else "the required count could not be read"
    lines = [
        f"Review ask: {ref['owner']}/{ref['repo']}#{ref['number']} {pr.get('title', '')}".rstrip(),
        f"This PR needs {needed} approvals on {base} ({why_count}; never fewer than {MIN_PEOPLE}). Please review:",
        *(f"- @{p['login']} ({p['why']}): {_what(p['paths'], files)}" for p in people),
    ]
    if groups:
        lines.append("FYI, not counted toward approvals: " + ", ".join(f"@{g}" for g in sorted(groups)))
    if status == "short":
        lines.append(f"Missing: {needed - len(people)} more person(s). Add them with --reviewer <login>.")
    text = "\n".join(lines) + "\n"
    events.emit("work.review_ask", status=status, needed=needed, named=len(people), groups=len(groups), files=len(files), source=req["source"])
    return {"status": status, "needed": needed, "required": req["count"], "source": req["source"], "people": people, "groups": sorted(groups), "files": len(files), "text": text, "author": author}


def check(text: str, needed: int, author: str = "") -> dict[str, Any]:
    """Count the people a draft names. Refused (`ok` False) when fewer than max(needed, 2); group handles never count."""
    handles = list(dict.fromkeys(MENTION.findall(text)))
    people = list(dict.fromkeys(h.lower() for h in handles if not is_group(h) and h.lower() not in {author.lower(), *CHAT_WIDE}))  # @Ana and @ana are one person
    groups = [h for h in handles if is_group(h)]
    need = max(needed, MIN_PEOPLE)
    ok = len(people) >= need
    events.emit("work.review_ask.check", status="ok" if ok else "refused", needed=need, named=len(people), groups=len(groups))
    return {"ok": ok, "needed": need, "people": len(people), "groups": len(groups)}


def check_pr(url: str, text: str) -> dict[str, Any]:
    """`check` against the PR's own required approvals and author."""
    _ref, repo_api, hdr, pr = _open(url)
    req = required_approvals(repo_api, (pr.get("base") or {}).get("ref") or "main", hdr)
    return check(text, req["count"], (pr.get("user") or {}).get("login", ""))
