"""Read-only connectors to work tools: GitHub, Jira, Slack, Google Docs (Drive), Zoom, Outlook, PagerDuty.

Each connector turns the user's own activity into evidence items (business
data; see privacy.py). Configuration lives in `<data root>/work/connectors.yaml`
or a plugin's `evidence_sources`:

  connectors:
    - {kind: github, token: keychain:careerbrew.github, api: https://api.github.com, user: you}
    - {kind: jira, base_url: https://jira.example.com, token: op://Work/Jira/token, email: you@example.com}
    - {kind: slack, token: keychain:careerbrew.slack}          # user token, scope search:read
    - {kind: gdocs, token: env:GOOGLE_OAUTH_TOKEN}             # scope drive.metadata.readonly
    - {kind: zoom, token: keychain:careerbrew.zoom}            # scope meeting:read
    - {kind: outlook, auth: device, client_id: <app>, tenant: organizations, token: keychain:careerbrew.outlook.refresh}  # Graph, delegated; msauth.py refreshes it
    - {kind: pagerduty, token: keychain:careerbrew.pagerduty}  # a user API token; reads /users/me, incidents, your log entries

The outlook connector also feeds the day calendar (calendar.py) and mail metadata (mail.py).
Slack, PagerDuty, and Outlook also feed the work feed (`FEED_READERS`, work/feed.py): asks of you,
pages, mail, meetings, and your own actions, read over the last `feed_days` days (default 2).

Security:
- tokens are references (secrets.py), resolved per call, never stored;
- every request goes through `http_json`, which enforces the plugin policy
  `allowed_hosts` and `allowed_connectors` (corporate machines);
- evidence keeps only metadata (titles, dates, links, counts), never message bodies. The work
  feed keeps the original event (a Slack message's text) encrypted and apart from its analysis,
  for `feed_keep_days` days (ADR 0008);
- an event that is personal (a private calendar entry, a rule, the owner's mark) never becomes evidence.

Tests and e2e runs set `CAREERBREW_HTTP_FIXTURES=<dir>`. Then requests read
`<dir>/<host>/<path>.json` instead of the network.
"""

from __future__ import annotations

import base64
import json
import os
import re
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from . import CONTACT, events, plugins, secrets
from .work.evidence import tags_for

KINDS = ("github", "jira", "slack", "gdocs", "zoom", "outlook", "pagerduty")
GRAPH_API = "https://graph.microsoft.com/v1.0"
PAGERDUTY_API = "https://api.pagerduty.com"
FEED_DAYS = 2  # how far back a feed read looks (`feed_days` on the connector changes it)
PERSONAL_SENSITIVITY = ("personal", "private")  # Outlook's own marks for a private entry
JIRA_MAX_PAGES = 50


class ConnectorError(RuntimeError):
    pass


def _fixture_path(url: str, ext: str = ".json") -> Path:
    u = urllib.parse.urlsplit(url)
    key = re.sub(r"[^A-Za-z0-9_.-]+", "_", u.path.strip("/")) or "root"
    return Path(os.environ["CAREERBREW_HTTP_FIXTURES"]) / u.netloc / f"{key}{ext}"


def http_get(url: str, headers: dict[str, str] | None = None) -> bytes:
    """Raw GET (RSS, JSON arrays, CSV) with the same host policy and fixture support as http_json."""
    check_host(url)
    host = urllib.parse.urlsplit(url).hostname or ""
    if os.environ.get("CAREERBREW_HTTP_FIXTURES"):
        for ext in (".json", ".xml", ".html", ".csv"):
            p = _fixture_path(url, ext)
            if p.exists():
                return p.read_bytes()
        raise FileNotFoundError(f"no fixture for {host}{urllib.parse.urlsplit(url).path}")
    req = urllib.request.Request(url, headers={"User-Agent": f"careerbrew/0.2 (personal job search; contact via {CONTACT})", **(headers or {})})
    with open_url(req, timeout=30) as resp:
        return resp.read()


LOCAL_HOSTS = {"127.0.0.1", "localhost", "::1"}


def host_allowed(host: str) -> bool:
    if host in LOCAL_HOSTS:
        return True  # local services such as Proton Mail Bridge
    allowed = plugins.effective_policy().get("allowed_hosts")
    return allowed is None or any(host == h or host.endswith("." + h) for h in allowed)


def check_host(url_or_host: str) -> None:
    host = urllib.parse.urlsplit(url_or_host).hostname if "://" in url_or_host else url_or_host
    if not host_allowed(host or ""):
        raise ConnectorError(f"host {host} is not in the plugin policy allowed_hosts")


# Headers that carry no secret. A request with any other header (a token, a cookie, a custom key) never follows a
# redirect to another origin: urllib would send the same headers to the new address.
_PLAIN_HEADERS = {"user-agent", "accept", "accept-language", "content-type", "content-length", "host"}


def _origin(url: str) -> tuple[str, str, int]:
    u = urllib.parse.urlsplit(url)
    port = u.port if u.port is not None else {"http": 80, "https": 443}.get(u.scheme.lower(), -1)  # port 0 is its own origin
    return u.scheme.lower(), (u.hostname or "").lower(), port


class _PolicyRedirect(urllib.request.HTTPRedirectHandler):
    """Check each redirect target like the first address: the host policy, then the caller's own check.

    Also refuse a move from HTTPS to plain HTTP, and a move to another origin for a request that carries a token."""

    def __init__(self, also: Callable[[str], None] | None = None):
        self.also = also

    def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]
        check_host(newurl)
        if self.also:
            self.also(newurl)
        old, new = _origin(req.full_url), _origin(newurl)
        if old[0] == "https" and new[0] != "https":
            raise ConnectorError(f"redirect from HTTPS to plain HTTP refused ({new[1]})")
        if old != new and any(name.lower() not in _PLAIN_HEADERS for name, _ in req.header_items()):
            raise ConnectorError(f"redirect to another address refused for a request that carries a token ({new[1]})")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def open_url(req: urllib.request.Request, timeout: float, also: Callable[[str], None] | None = None) -> Any:
    """Open a request whose first host the caller checked. A redirect to a host outside the policy, to plain HTTP from HTTPS, or to another origin with a token raises ConnectorError."""
    return urllib.request.build_opener(_PolicyRedirect(also)).open(req, timeout=timeout)  # noqa: S310 (policy-checked hosts)


def http_json(url: str, headers: dict[str, str], body: dict[str, Any] | None = None) -> Any:
    check_host(url)
    if os.environ.get("CAREERBREW_HTTP_FIXTURES"):
        fixture = json.loads(_fixture_path(url).read_text(encoding="utf-8"))
        if isinstance(fixture, dict) and "__status" in fixture:  # a fixture can stand for an HTTP error
            raise urllib.error.HTTPError(url, int(fixture["__status"]), "fixture", {}, None)  # type: ignore[arg-type]
        return fixture
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers={"Accept": "application/json", "Content-Type": "application/json", **headers})
    with open_url(req, timeout=30) as resp:
        return json.loads(resp.read())


def _since(cfg: dict[str, Any]) -> datetime:
    from .calendar import now_utc  # local: calendar imports this module

    return now_utc() - timedelta(days=int(cfg.get("days", 365)))


def _item(origin: str, kind: str, iid: str, title: str, at: str, url: str = "", tags: list[str] | None = None) -> dict[str, Any]:
    return {"id": f"{kind}:{iid}", "kind": kind, "title": title[:200], "at": at, "url": url, "tags": sorted(set(tags or []) | set(tags_for(title))), "origin": origin}


def github(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    api = cfg.get("api", "https://api.github.com").rstrip("/")
    tok = secrets.resolve(cfg["token"])
    q = urllib.parse.quote(f"author:{cfg['user']} type:pr created:>={_since(cfg).date()}")
    data = http_json(f"{api}/search/issues?q={q}&per_page=100", {"Authorization": f"Bearer {tok}"})
    out = []
    for pr in data.get("items", []):
        merged_at = pr.get("pull_request", {}).get("merged_at")
        tags = ["delivery"] if merged_at else []
        out.append({**_item(origin, "github", str(pr["id"]), pr["title"], pr["created_at"], pr.get("html_url", ""), tags), "draft": pr.get("draft") is True, "state": pr.get("state"), "merged_at": merged_at})
    # Reviews submitted today, keyed by the day (issues #44, #52): the search finds PRs that changed
    # today; only reviews by the user with submitted_at on that day count.
    from .calendar import now as clock_now

    now = clock_now().astimezone(UTC)
    day = now.date().isoformat()
    found = http_json(f"{api}/search/issues?q={urllib.parse.quote(f'reviewed-by:{cfg["user"]} type:pr updated:>={day}')}&per_page=50", {"Authorization": f"Bearer {tok}"})
    count = 0
    for pr in found.get("items", []):
        if not pr.get("repository_url") or not pr.get("number"):
            continue
        reviews = http_json(f"{pr['repository_url']}/pulls/{pr['number']}/reviews?per_page=100", {"Authorization": f"Bearer {tok}"})
        if any((r.get("user") or {}).get("login") == cfg["user"] and str(r.get("submitted_at", "")).startswith(day) for r in reviews or []):
            count += 1
    if count:
        out.append(_item(origin, "github", f"reviews-{day}", f"Reviewed {count} pull requests", now.isoformat(timespec="seconds"), tags=["collaboration"]))
    return out


def jira(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    base = cfg["base_url"].rstrip("/")
    tok = secrets.resolve(cfg["token"])
    auth = _jira_auth(cfg.get("email", ""), tok)
    # An unauthenticated search answers 200 with 0 issues, so prove the token first (issue #19).
    try:
        http_json(f"{base}/rest/api/3/myself", {"Authorization": auth})
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise ConnectorError(f"jira rejected the token ({e.code} on /rest/api/3/myself) (fix: check the email and token; a scoped token needs base_url https://api.atlassian.com/ex/jira/<cloudId>)") from None
        raise
    jql = cfg.get("jql", f"assignee = currentUser() AND statusCategory = Done AND resolved >= -{int(cfg.get('days', 365))}d")
    issues: list[dict[str, Any]] = []
    page_token = ""
    for _ in range(JIRA_MAX_PAGES):
        url = f"{base}/rest/api/3/search/jql?jql={urllib.parse.quote(jql)}&fields=summary,resolutiondate,issuetype,priority&maxResults=100"
        data = http_json(url + (f"&nextPageToken={urllib.parse.quote(page_token)}" if page_token else ""), {"Authorization": auth})
        issues.extend(data.get("issues", []))
        page_token = data.get("nextPageToken") or ""
        if data.get("isLast", True) or not page_token:
            break
    out = []
    for issue in issues:
        f = issue.get("fields", {})
        itype = (f.get("issuetype") or {}).get("name", "").lower()
        tags = {"bug": ["quality"], "incident": ["ownership"], "epic": ["impact"], "story": ["delivery"], "task": ["delivery"]}.get(itype, ["delivery"])
        out.append(_item(origin, "jira", issue["key"], f"{issue['key']}: {f.get('summary', '')}", f.get("resolutiondate") or "", f"{base}/browse/{issue['key']}", tags))
    return out


def slack(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    """Recognition signals only: messages that thank or mention you. Bodies are not kept."""
    tok = secrets.resolve(cfg["token"])
    query = cfg.get("query", "thanks OR kudos OR shoutout to:me")
    data = http_json(f"https://slack.com/api/search.messages?query={urllib.parse.quote(query)}&count=100", {"Authorization": f"Bearer {tok}"})
    if not data.get("ok", False):
        raise ConnectorError(f"slack error: {data.get('error', 'unknown')} (fix: a user token with search:read)")
    from .work import feed

    personal = feed.personal_check(origin)  # a message the feed keeps out as personal is not recognition either (issue #72)
    out = []
    for m in (data.get("messages") or {}).get("matches", []):
        if personal(_slack_item(m, "")):
            continue
        when = datetime.fromtimestamp(float(m["ts"]), UTC).isoformat(timespec="seconds")
        channel = (m.get("channel") or {}).get("name", "dm")
        out.append(_item(origin, "slack", m["ts"], f"Recognition in #{channel}", when, m.get("permalink", ""), ["collaboration"]))
    return out


def gdocs(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    tok = secrets.resolve(cfg["token"])
    q = f"'me' in owners and mimeType='application/vnd.google-apps.document' and modifiedTime > '{_since(cfg).strftime('%Y-%m-%dT%H:%M:%S')}'"
    data = http_json(
        f"https://www.googleapis.com/drive/v3/files?q={urllib.parse.quote(q)}&fields=files(id,name,createdTime,webViewLink)&pageSize=200",
        {"Authorization": f"Bearer {tok}"},
    )
    out = []
    for f in data.get("files", []):
        low = f["name"].lower()
        tags = ["design"] if any(w in low for w in ("design", "rfc", "proposal", "adr", "architecture")) else ["communication"]
        if "postmortem" in low or "incident" in low:
            tags = ["ownership"]
        out.append(_item(origin, "gdocs", f["id"], f["name"], f.get("createdTime", ""), f.get("webViewLink", ""), tags))
    return out


def zoom(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    """Meetings you hosted (topic + date): signals for leadership and collaboration."""
    tok = secrets.resolve(cfg["token"])
    data = http_json(f"https://api.zoom.us/v2/users/me/meetings?type=previous_meetings&page_size=300&from={_since(cfg).date()}", {"Authorization": f"Bearer {tok}"})
    out = []
    for m in data.get("meetings", []):
        topic = m.get("topic", "Meeting")
        out.append(_item(origin, "zoom", str(m["uuid"]), f"Hosted: {topic}", m.get("start_time", ""), tags=_meeting_tags(topic)))
    return out


def _meeting_tags(topic: str) -> list[str]:
    low = topic.lower()
    if any(w in low for w in ("design review", "architecture", "rfc")):
        return ["design"]
    return ["leadership"] if any(w in low for w in ("1:1", "mentor", "onboard", "interview")) else ["collaboration"]


def _graph(cfg: dict[str, Any], path: str) -> Any:
    """One Microsoft Graph GET with the outlook connector's token and api, times in UTC."""
    from . import msauth  # local: msauth imports this module

    tok = msauth.bearer(cfg)
    return http_json(cfg.get("api", GRAPH_API).rstrip("/") + path, {"Authorization": f"Bearer {tok}", "Prefer": 'outlook.timezone="UTC"'})


def outlook(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    """Meetings you organized in Outlook (subject + date, Microsoft Graph calendarView). No bodies, no attendees, no private entries."""
    from .calendar import now as clock_now

    end = clock_now().astimezone(UTC)
    start = end - timedelta(days=int(cfg.get("days", 365)))
    fmt = "%Y-%m-%dT%H:%M:%SZ"
    data = _graph(cfg, f"/me/calendarView?startDateTime={start.strftime(fmt)}&endDateTime={end.strftime(fmt)}&$select=id,subject,start,isOrganizer,isCancelled,sensitivity&$top=500")
    out = []
    for ev in data.get("value", []):
        if not ev.get("isOrganizer") or ev.get("isCancelled") or ev.get("sensitivity") in PERSONAL_SENSITIVITY:
            continue
        subject = ev.get("subject") or "Meeting"
        at = (ev.get("start") or {}).get("dateTime", "")[:19] + "Z"  # Prefer: UTC, so mark it (promo compares aware times)
        out.append(_item(origin, "outlook", str(ev.get("id", at)), f"Organized: {subject}", at, tags=_meeting_tags(subject)))
    return out


def graph_sources(kind: str) -> list[dict[str, Any]]:
    """Outlook connectors as calendar or mail sources (`kind` graph), unless plugin policy disallows outlook."""
    allowed = plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and "outlook" not in allowed:
        return []
    from .work.evidence import sources_list

    out = []
    for c in sources_list():
        if c.get("kind") == "outlook" and c.get("token"):
            src = {"kind": "graph", "token": c["token"], "api": c.get("api", GRAPH_API), "name": "outlook"}
            src.update({k: c[k] for k in ("auth", "client_id", "tenant") if c.get(k)})  # a device sign-in refreshes its token (issue #199)
            if kind == "mail":
                src["machine"] = "any" if c.get("origin") == "user" else "corporate"
            out.append(src)
    return out


# ---------- work feed readers (issue #72) ----------
# Each returns events in the feed's shape (work/feed.py `clean`): facts for the analysis, plus the
# text and the tool's payload, which the feed keeps only in its encrypted originals.


def _feed_window(cfg: dict[str, Any], days: int | None = None) -> tuple[datetime, datetime]:
    from .calendar import now as clock_now

    end = clock_now().astimezone(UTC)
    return end - timedelta(days=int(days or cfg.get("feed_days", FEED_DAYS))), end


def _iso(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _slack_item(m: dict[str, Any], me: str, thread: str = "") -> dict[str, Any]:
    """One Slack message as a feed event. `me` is your user id (empty when unknown); `thread` is the parent
    event's id for a thread reply (issue #180), else the thread is read from the permalink."""
    ch = m.get("channel") or {}
    text, user = str(m.get("text") or ""), str(m.get("user") or "")
    actor = "me" if me and user == me else "bot" if m.get("bot_id") or m.get("subtype") == "bot_message" or not user else "other"
    dm = bool(ch.get("is_im") or ch.get("is_mpim"))
    mention = bool(me) and f"<@{me}>" in text
    name = str(m.get("username") or user or "someone")
    where = "a DM" if dm else f"#{ch.get('name', 'channel')}"
    if actor == "me":
        title = f"You wrote in {where}"
    else:
        title = f"DM from {name}" if dm else f"Mention from {name} in {where}" if mention else f"{name} in {where}"
    permalink = str(m.get("permalink") or "")
    root = (urllib.parse.parse_qs(urllib.parse.urlsplit(permalink).query).get("thread_ts") or [""])[0] or str(m["ts"])
    cid = str(ch.get("id") or ch.get("name") or "")
    return {
        "id": f"slack:{cid}:{m['ts']}",
        "source": "slack",
        "kind": "dm" if dm else "mention" if mention else "message",
        "at": datetime.fromtimestamp(float(m["ts"]), UTC).isoformat(timespec="seconds"),
        "title": title,
        "actor": actor,
        "actor_id": user,  # the chat user id: matched against your team roster (issue #180)
        "to_me": (dm or mention) and actor != "me",
        "channel": "" if dm else str(ch.get("name") or ""),
        "from": name,
        "url": permalink,
        "thread": thread or (f"slack:{cid}" if dm else f"slack:{cid}:{root}"),  # a DM is one conversation; a channel ask, its thread
        "text": text,
        "original": m,
    }


def slack_feed(cfg: dict[str, Any]) -> list[dict[str, Any]]:
    """Messages to you (DMs, mentions) and your own messages, from Slack search (`feed_queries`, default to:me and from:me);
    then, best effort, some teammates' messages and mentions and the replies of the threads found (issue #180)."""
    hdr = {"Authorization": f"Bearer {secrets.resolve(cfg['token'])}"}
    who = http_json("https://slack.com/api/auth.test", hdr)
    if not who.get("ok", False):
        raise ConnectorError(f"slack error: {who.get('error', 'unknown')} (fix: a user token with search:read)")
    me = str(who.get("user_id") or "")
    start, _end = _feed_window(cfg)
    after = (start - timedelta(days=1)).date().isoformat()  # Slack's after: excludes the day itself
    found: dict[str, dict[str, Any]] = {}
    roots: dict[str, dict[str, Any]] = {}

    def search(q: str) -> dict[str, Any]:
        return http_json(f"https://slack.com/api/search.messages?query={urllib.parse.quote(f'{q} after:{after}')}&count=100&sort=timestamp", hdr)

    def keep(data: dict[str, Any]) -> None:
        for m in (data.get("messages") or {}).get("matches", []):
            if m.get("ts"):
                item = _slack_item(m, me)
                found[item["id"]] = item
                if item["thread"] == item["id"]:  # a thread's first message: read its replies too
                    roots[item["id"]] = m

    for q in cfg.get("feed_queries") or ["to:me", "from:me"]:
        data = search(q)
        if not data.get("ok", False):
            raise ConnectorError(f"slack error: {data.get('error', 'unknown')} (fix: a user token with search:read)")
        keep(data)
    for q in _team_queries():  # extra: a rate limit or an error stops them and keeps what was found
        try:
            data = search(q)
        except (RuntimeError, OSError, ValueError) as exc:
            events.emit("work.feed.team_search", status="error", error=type(exc).__name__, story="us-78")
            break
        if not data.get("ok", False):
            events.emit("work.feed.team_search", status="error", error=_slack_code(data), story="us-78")
            break
        keep(data)
    base = str(who.get("url") or "").rstrip("/")
    for item in _slack_replies(hdr, me, roots, base):
        selected = found.setdefault(item["id"], item)
        if selected["thread"] == item["id"] != item["thread"]:
            # Search can omit the parent from its permalink. Keep its fields
            # but use the known parent from this same reply in the thread.
            query = urllib.parse.urlsplit(selected["url"]).query
            if not urllib.parse.parse_qs(query).get("thread_ts"):
                selected["thread"] = item["thread"]
    return list(found.values())


def _slack_code(data: dict[str, Any]) -> str:
    """Slack's error code (`missing_scope`, `ratelimited`), kept to its short id shape so no text reaches the log."""
    code = str(data.get("error") or "unknown")
    return code if re.fullmatch(r"[a-z_]{1,40}", code) else "unknown"


# Slack's search is rate-limited to about 20 calls a minute: a pull searches TEAM_PER_PULL teammates (two calls
# each), the next group on the next 15-minute pull, so the whole roster is covered in turn with no saved cursor.
TEAM_PER_PULL = 5
THREADS_MAX = 30  # threads read per pull, newest first; each costs one conversations.replies call (more for long threads)
REPLY_PAGES = 3  # pages of 100 replies read per thread
# Errors that are the same for every thread: stop reading replies; so does missing_scope on a public channel
# (no channels:history). Any other (a deleted thread, a private channel without groups:history) skips that thread.
SLACK_FATAL = {"ratelimited", "invalid_auth", "not_authed", "token_revoked", "account_inactive"}


def _team_queries() -> list[str]:
    """Slack searches for this pull's group of teammates (issue #180): `from:<@ID>` and `<@ID>` for each."""
    from . import calendar
    from .work import team

    ids = [r["chat"] for r in team.roster() if r["chat"]]
    if not ids:
        return []
    turn = int(calendar.now_utc().timestamp() // 900)  # the 15-minute pull this is
    groups = max(1, -(-len(ids) // TEAM_PER_PULL))
    first = (turn % groups) * TEAM_PER_PULL
    return [q for i in ids[first : first + TEAM_PER_PULL] for q in (f"from:<@{i}>", f"<@{i}>")]


def _slack_link(base: str, cid: str, ts: str, parent_ts: str) -> str:
    """A reply's own link, built from its ids, so it matches the one search gives (empty when the workspace URL is unknown)."""
    return f"{base}/archives/{cid}/p{ts.replace('.', '')}?thread_ts={parent_ts}&cid={cid}" if base else ""


def _slack_replies(hdr: dict[str, str], me: str, roots: dict[str, dict[str, Any]], base: str = "") -> list[dict[str, Any]]:
    """Every reply of each thread found (newest threads first, at most THREADS_MAX), as its own event whose `thread`
    is the parent's id, so a thread is read in full. Needs `channels:history` (and `groups:history` for private
    channels). A failed read never loses the pull: it is a content-free `work.feed.replies` event."""
    out: list[dict[str, Any]] = []
    skipped = 0
    for parent_id, m in sorted(roots.items(), key=lambda kv: -float(kv[1]["ts"]))[:THREADS_MAX]:
        ch = m.get("channel") or {}
        if ch.get("is_im") or ch.get("is_mpim") or not ch.get("id"):
            continue  # a DM is one conversation already
        cid, cursor = str(ch["id"]), ""
        for _page in range(REPLY_PAGES):
            url = f"https://slack.com/api/conversations.replies?channel={urllib.parse.quote(cid)}&ts={urllib.parse.quote(str(m['ts']))}&limit=100"
            if cursor:
                url += f"&cursor={urllib.parse.quote(cursor)}"
            try:
                data = http_json(url, hdr)
            except (RuntimeError, OSError, ValueError) as exc:  # a 429 or a timeout: stop, keep what was read
                events.emit("work.feed.replies", status="error", error=type(exc).__name__, count=len(out), story="us-78")
                return out
            if not data.get("ok", False):
                code = _slack_code(data)
                if code in SLACK_FATAL or code == "missing_scope" and not ch.get("is_private"):  # no channels:history: no thread reads
                    events.emit("work.feed.replies", status="error", error=code, count=len(out), story="us-78")
                    return out
                skipped += 1  # this thread only: deleted, or a channel the token cannot read
                break
            for r in data.get("messages") or []:
                if r.get("ts") and str(r["ts"]) != str(m["ts"]):
                    link = _slack_link(base, cid, str(r["ts"]), str(m["ts"]))
                    out.append(_slack_item({**r, "channel": ch, "permalink": link}, me, thread=parent_id))
            cursor = str(((data.get("response_metadata") or {}).get("next_cursor")) or "")
            if not data.get("has_more") or not cursor:
                break
    if roots:
        events.emit("work.feed.replies", status="ok", count=len(out), skipped=skipped, story="us-78")
    return out


def _pd(cfg: dict[str, Any]) -> tuple[str, dict[str, str], str]:
    """PagerDuty base URL, headers, and your user id (a user API token answers /users/me)."""
    api = cfg.get("api", PAGERDUTY_API).rstrip("/")
    hdr = {"Authorization": f"Token token={secrets.resolve(cfg['token'])}", "Accept": "application/vnd.pagerduty+json;version=2"}
    me = str(((http_json(f"{api}/users/me", hdr) or {}).get("user") or {}).get("id") or "")
    if not me:
        raise ConnectorError("pagerduty: /users/me has no user (fix: use a user API token, not an account token)")
    return api, hdr, me


PD_ACTIONS = {"acknowledge_log_entry": ("Acknowledged page", "acknowledged"), "resolve_log_entry": ("Resolved incident", "resolved")}
PD_MAX_PAGES = 10


def _pd_actions(api: str, hdr: dict[str, str], me: str, start: datetime, end: datetime) -> list[dict[str, Any]]:
    """Pages you acknowledged or resolved (your log entries), every page of results up to PD_MAX_PAGES."""
    out = []
    offset = 0
    for _ in range(PD_MAX_PAGES):
        data = http_json(f"{api}/users/{urllib.parse.quote(me)}/log_entries?since={_iso(start)}&until={_iso(end)}&time_zone=UTC&is_overview=true&limit=100&offset={offset}", hdr)
        entries = data.get("log_entries", [])
        for e in entries:
            verb = PD_ACTIONS.get(str(e.get("type")))
            if verb is None or (e.get("agent") or {}).get("id") != me:
                continue
            inc = e.get("incident") or {}
            summary = str(inc.get("summary") or "an incident")
            out.append(
                {
                    "id": f"pagerduty:{e['id']}",
                    "source": "pagerduty",
                    "kind": "incident-action",
                    "at": e.get("created_at", ""),
                    "title": f"{verb[0]}: {summary}",
                    "actor": "me",
                    "status": verb[1],
                    "url": str(inc.get("html_url") or ""),
                    "thread": f"pagerduty:{inc.get('id', '')}",
                    "tags": ["ownership"],
                    "text": summary,
                    "original": e,
                }
            )
        if not data.get("more"):
            break
        offset += len(entries) or 100
    return out


def pagerduty_feed(cfg: dict[str, Any]) -> list[dict[str, Any]]:
    """Incidents assigned to you (each one asks you) and the pages you acknowledged or resolved."""
    api, hdr, me = _pd(cfg)
    start, end = _feed_window(cfg)
    q = f"user_ids%5B%5D={urllib.parse.quote(me)}&since={_iso(start)}&until={_iso(end)}&time_zone=UTC&limit=100"
    out = []
    for inc in http_json(f"{api}/incidents?{q}", hdr).get("incidents", []):
        out.append(
            {
                "id": f"pagerduty:{inc['id']}",
                "source": "pagerduty",
                "kind": "page",
                "at": inc.get("created_at", ""),
                "title": f"Page: {inc.get('title') or inc.get('summary') or 'an incident'}",
                "actor": "other",
                "to_me": True,  # the query returns only incidents assigned to you
                "channel": str((inc.get("service") or {}).get("summary") or ""),
                "status": str(inc.get("status") or ""),
                "importance": str(inc.get("urgency") or ""),
                "url": str(inc.get("html_url") or ""),
                "thread": f"pagerduty:{inc['id']}",
                "tags": ["ownership"],
                "text": str(inc.get("title") or ""),
                "original": inc,
            }
        )
    return out + _pd_actions(api, hdr, me, start, end)


def pagerduty(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    """Evidence: pages you acknowledged or resolved (incident title + date), by the work feed's rules and marks."""
    from .work import feed

    api, hdr, me = _pd(cfg)
    start, end = _feed_window(cfg, days=int(cfg.get("days", 365)))
    return feed.evidence_from(_pd_actions(api, hdr, me, start, end), origin)


_NO_REPLY = re.compile(r"^(?:no-?reply|do-?not-?reply|notifications?|alerts?|mailer-daemon|bounces?)(?:[+._-].*)?$")


def outlook_feed(cfg: dict[str, Any]) -> list[dict[str, Any]]:
    """Mail metadata (subject, sender, importance, flag; never bodies) and meetings, from the same Graph token as the outlook connector."""
    me = _graph(cfg, "/me")
    mine = {str(me.get(k) or "").lower() for k in ("mail", "userPrincipalName")} - {""}
    start, end = _feed_window(cfg)
    out = []
    cal = _graph(cfg, f"/me/calendarView?startDateTime={_iso(start)}&endDateTime={_iso(end)}&$select=id,subject,start,isOrganizer,isCancelled,sensitivity,organizer,webLink&$top=200")
    for ev in cal.get("value", []):
        if ev.get("isCancelled") or not ev.get("id"):
            continue
        subject, org = str(ev.get("subject") or "Meeting"), bool(ev.get("isOrganizer"))
        out.append(
            {
                "id": f"outlook:{ev['id']}",
                "source": "outlook",
                "kind": "meeting",
                "at": str((ev.get("start") or {}).get("dateTime", ""))[:19] + "Z",
                "title": f"Organized: {subject}" if org else f"Meeting: {subject}",
                "actor": "me" if org else "other",
                "to_me": not org,
                "from": str(((ev.get("organizer") or {}).get("emailAddress") or {}).get("name") or ""),
                "url": str(ev.get("webLink") or ""),
                "thread": f"outlook:{ev['id']}",
                "personal": ev.get("sensitivity") in PERSONAL_SENSITIVITY,
                "tags": _meeting_tags(subject),
                "text": subject,
                "original": ev,
            }
        )
    since = _iso(start)
    mail = _graph(cfg, f"/me/messages?$select=id,conversationId,subject,from,toRecipients,receivedDateTime,importance,flag,webLink&$top=100&$orderby=receivedDateTime%20desc&$filter=receivedDateTime%20ge%20{since}")
    for m in mail.get("value", []):
        sender = (m.get("from") or {}).get("emailAddress") or {}
        addr, subject = str(sender.get("address") or "").lower(), str(m.get("subject") or "(no subject)")
        actor = "me" if addr in mine else "bot" if _NO_REPLY.match(addr.split("@")[0]) else "other"
        to = {str((r.get("emailAddress") or {}).get("address") or "").lower() for r in m.get("toRecipients") or []}
        out.append(
            {
                "id": f"outlook:mail:{m['id']}",
                "source": "outlook",
                "kind": "mail",
                "at": m.get("receivedDateTime", ""),
                "title": f"You sent: {subject}" if actor == "me" else f"Mail from {sender.get('name') or addr}: {subject}",
                "actor": actor,
                "to_me": bool(to & mine) and actor != "me",
                "from": addr,
                "status": "flagged" if (m.get("flag") or {}).get("flagStatus") == "flagged" else "",
                "importance": str(m.get("importance") or ""),
                "url": str(m.get("webLink") or ""),
                "thread": f"outlook:conv:{m.get('conversationId') or m['id']}",
                "text": subject,
                "original": m,
            }
        )
    return out


FEED_READERS: dict[str, Callable[[dict[str, Any]], list[dict[str, Any]]]] = {"slack": slack_feed, "pagerduty": pagerduty_feed, "outlook": outlook_feed}


def feed(cfg: dict[str, Any]) -> list[dict[str, Any]]:
    """Read one connector's events for the work feed, under the same policy checks as `run`."""
    kind = cfg["kind"]
    allowed = plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and kind not in allowed:
        raise ConnectorError(f"connector {kind} is not allowed by plugin policy (allowed: {allowed})")
    secrets.backend_of(cfg["token"])  # rejects a literal token in config
    items = FEED_READERS[kind](cfg)
    events.emit("connector.feed", source_kind=kind, status="ok", items=len(items))
    return items


# What `careerbrew connect` prints, so a user with no prior knowledge can create each token (issue #18).
GUIDE: dict[str, dict[str, Any]] = {
    "jira": {
        "token_page": "https://id.atlassian.com/manage-profile/security/api-tokens",
        "looks_like": "a long string that often starts with ATATT (Cloud) or a personal access token (Data Center)",
        "needs": ["base_url", "email"],
        "scope": "read:jira-work and read:jira-user (a scoped token works only through https://api.atlassian.com/ex/jira/<cloudId>; careerbrew finds it)",
    },
    "github": {"token_page": "https://github.com/settings/personal-access-tokens/new", "looks_like": "github_pat_... or ghp_...", "needs": ["user"], "scope": "read-only: Pull requests, Metadata"},
    "slack": {"token_page": "https://api.slack.com/apps (your app > OAuth & Permissions)", "looks_like": "xoxp-...", "needs": [], "scope": "search:read, plus chat:write to send the PM update"},
    "gdocs": {"token_page": "the place your admin names (use only a token from the OAuth client your admin approved)", "looks_like": "ya29....", "needs": [], "scope": "drive.metadata.readonly"},
    "zoom": {
        "token_page": "https://marketplace.zoom.us (Develop > Build App > Server-to-Server OAuth; a Zoom account admin must create or approve the app)",
        "looks_like": "a JWT-like access token",
        "needs": [],
        "scope": "meeting:read",
    },
    "outlook": {
        "token_page": "the place your admin names (manual token only, with --paste: use only a token your admin issued or allows)",
        "looks_like": "a long JWT that starts with eyJ0 (a manual token expires after about an hour; the device sign-in, the default, keeps working for weeks)",
        "needs": [],
        "scope": "delegated, read-only: User.Read, Calendars.ReadBasic, Mail.ReadBasic",
    },
    "pagerduty": {
        "token_page": "https://<your-subdomain>.pagerduty.com/users (My Profile > User Settings > Create API User Token)",
        "looks_like": "a 20-character user API token (an account-wide token cannot answer /users/me)",
        "needs": [],
        "scope": "a user token; careerbrew only reads /users/me, incidents assigned to you, and your own log entries",
    },
}


def plugin_defaults(kind: str) -> dict[str, Any]:
    """Connector defaults from installed plugins (`connector_defaults: {jira: {base_url}, github: {api, token_page}}`); the first plugin that sets a key wins."""
    out: dict[str, Any] = {}
    for p in plugins.installed():
        for key, val in ((p["manifest"].get("connector_defaults") or {}).get(kind) or {}).items():
            out.setdefault(key, val)
    return out


def token_page(kind: str, api: str | None = None, defaults: dict[str, Any] | None = None) -> str:
    """Where to create the token: a plugin's token_page, else the Enterprise host behind --api, else the public page (issue #38)."""
    defaults = defaults or {}
    if defaults.get("token_page"):
        return str(defaults["token_page"])
    if kind == "github" and api:
        host = urllib.parse.urlsplit(api).hostname or ""
        if host and host != "api.github.com":
            return f"https://{host}/settings/personal-access-tokens/new"
    return GUIDE[kind]["token_page"]


def _jira_auth(email: str, token: str) -> str:
    return "Basic " + base64.b64encode(f"{email}:{token}".encode()).decode() if email else f"Bearer {token}"


def verify(cfg: dict[str, Any], token: str) -> dict[str, Any]:
    """Call the tool's "who am I" endpoint. Returns {ok, message, base_url?}; never prints the token."""
    kind = cfg["kind"]
    try:
        if kind == "jira":
            base = cfg["base_url"].rstrip("/")
            auth = _jira_auth(cfg.get("email", ""), token)
            try:
                http_json(f"{base}/rest/api/3/myself", {"Authorization": auth})
                return {"ok": True, "message": "jira: token works on the site URL", "base_url": base}
            except urllib.error.HTTPError as e:
                if e.code not in (401, 403):
                    raise
            # A scoped Atlassian token is rejected on the site URL but works through the API gateway.
            try:
                cloud = http_json(f"{base}/_edge/tenant_info", {}).get("cloudId")
            except (urllib.error.HTTPError, ConnectorError, OSError):
                cloud = None
            if cloud:
                gateway = f"https://api.atlassian.com/ex/jira/{cloud}"
                try:
                    http_json(f"{gateway}/rest/api/3/myself", {"Authorization": auth})
                    return {"ok": True, "message": "jira: scoped token works through the API gateway; base_url set to it", "base_url": gateway}
                except urllib.error.HTTPError:
                    pass
            return {"ok": False, "message": "jira rejected the token on /rest/api/3/myself (fix: check the email is your Atlassian login, create a new token at " + GUIDE["jira"]["token_page"] + ", and paste it whole)"}
        if kind == "github":
            api = cfg.get("api", "https://api.github.com").rstrip("/")
            me = http_json(f"{api}/user", {"Authorization": f"Bearer {token}"})
            return {"ok": True, "message": f"github: token works for {me.get('login', 'your account')}"}
        if kind == "outlook":
            http_json(f"{cfg.get('api', GRAPH_API).rstrip('/')}/me", {"Authorization": f"Bearer {token}"})
            return {"ok": True, "message": "outlook: token works on Microsoft Graph /me"}
        if kind == "pagerduty":
            me = http_json(f"{cfg.get('api', PAGERDUTY_API).rstrip('/')}/users/me", {"Authorization": f"Token token={token}", "Accept": "application/vnd.pagerduty+json;version=2"})
            return {"ok": True, "message": f"pagerduty: token works for {(me.get('user') or {}).get('name', 'your user')}"}
    except urllib.error.HTTPError as e:
        return {"ok": False, "message": f"{kind} rejected the token ({e.code}) (fix: create a new one at {GUIDE[kind]['token_page']})"}
    except ConnectorError as e:
        return {"ok": False, "message": f"{e} (fix: for a host the plugin's owner approved, add it to the plugin's allowed_hosts, then careerbrew plugin refresh)"}
    except OSError as e:  # URLError, DNS, TLS, timeouts
        return {"ok": False, "message": f"{kind}: could not reach the server ({type(e).__name__}) (fix: check the URL and your VPN, or pass --no-verify)"}
    return {"ok": True, "message": f"{kind}: no check endpoint; the first `careerbrew work collect` tests it"}


def status(cfg: dict[str, Any]) -> dict[str, Any]:
    """Setup state of one connector, with the steps that finish it. Resolves keychain and env refs only
    (an `op://` read can prompt, so it is reported as unchecked)."""
    kind = cfg.get("kind", "")
    guide = GUIDE.get(kind, {})
    missing = [n for n in guide.get("needs", []) if not cfg.get(n)]
    ref = cfg.get("token", "")
    secret_ok: bool | None = None
    if ref and secrets.backend_of(ref) in ("env", "keychain"):
        try:
            secrets.resolve(ref)
            secret_ok = True
        except secrets.SecretError:
            secret_ok = False
    steps = []
    if kind == "outlook" and cfg.get("auth") == "device" and (secret_ok is False or _outlook_reconnect()):
        from . import msauth

        steps.append(msauth.RECONNECT + ".")
    elif secret_ok is False or not ref:
        steps.append(f"Create a token at {guide.get('token_page', 'the tool')} (it looks like {guide.get('looks_like', 'a token')}; scope: {guide.get('scope', 'read')}).")
        steps.append(f"Run in your terminal: careerbrew connect {kind}" + "".join(f" --{n.replace('_', '-')} <{n}>" for n in guide.get("needs", [])))
    elif missing:
        steps.append(f"Add {', '.join(missing)}: careerbrew connect {kind} " + " ".join(f"--{n.replace('_', '-')} <{n}>" for n in missing))
    return {"kind": kind, "origin": cfg.get("origin", "user"), "secret": secrets.backend_of(ref) if ref else None, "secret_ok": secret_ok, "missing": missing, "ready": not steps, "steps": steps}


def _outlook_reconnect() -> bool:
    from . import msauth

    return msauth.needs_reconnect()


REGISTRY: dict[str, Callable[[dict[str, Any], str], list[dict[str, Any]]]] = {"github": github, "jira": jira, "slack": slack, "gdocs": gdocs, "zoom": zoom, "outlook": outlook, "pagerduty": pagerduty}


def run(cfg: dict[str, Any], origin: str) -> list[dict[str, Any]]:
    kind = cfg["kind"]
    allowed = plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and kind not in allowed:
        raise ConnectorError(f"connector {kind} is not allowed by plugin policy (allowed: {allowed})")
    if "token" in cfg:
        secrets.backend_of(cfg["token"])  # rejects a literal token in config
    items = REGISTRY[kind](cfg, origin)
    events.emit("connector.run", source_kind=kind, status="ok", items=len(items))
    return items
