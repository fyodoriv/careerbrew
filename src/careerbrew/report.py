"""Run pages: each long careerbrew run ends with one HTML page of what was done, the blockers, and what to do next.

The page is self-contained: inline CSS (light and dark), no network, no JavaScript. It has three
sections, what needs you first: Blockers (why, and the fix), Next (each action with its exact
command), and Done (with evidence: counts, ids, PRs, the commands run). Long runs: `scripts/checkup.sh`, `careerbrew` with
no command, `review`, `work collect`, `day plan`, `day render`, `checkup`, `machine ready`, `machine new-user`,
and `plugin new` (`runs.py`). With pages on, a run prints only the page path and one summary line
(its key counts first); `careerbrew report latest` prints the page's items as text.

Beside each page sits a content-free summary (`<id>.json`): counts, ids, statuses, and commands
only. An agent reads it with `careerbrew report latest --json` and asks the owner which Next item
to take, without opening the page. An item's `detail` (a task title, a question's text) goes into
the page only, never into the summary.

Lifecycle: pages live in `<data root>/out/reports/` (plaintext by design, like every `out/` report;
git never tracks the data root). `latest.html` and `latest.json` are copies of the newest page. The
newest `KEEP_PER_RUN` pages of each run are kept; older ones are deleted when a new one is written.
`plugin remove` and a switch to the corporate role delete them with the rest of `out/`. The setting
`run_pages` (default on) turns pages off; then runs print their step text, as before.
"""

from __future__ import annotations

import html
import json
import re
import shutil
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from . import __version__, events, paths, privacy

KEEP_PER_RUN = 20
ID_RE = re.compile(r"^[a-z][a-z0-9-]{0,30}-\d{8}-\d{6}(?:-\d{1,9})?$")
RUN_RE = re.compile(r"^[a-z][a-z0-9-]{0,30}$")


@dataclass
class Item:
    """One line of a section. Every field but `detail` is content-free: careerbrew's own words, counts, ids, commands."""

    label: str
    id: str = ""
    evidence: str = ""  # Done: counts, ids, PRs
    why: str = ""  # Blockers: why it blocks
    command: str = ""  # Done: the command run; Blockers: the fix; Next: the exact command
    detail: str = ""  # page only: may hold business text (a task title), never in the summary


@dataclass
class Run:
    run: str  # short id: repo-checkup, start, review, work-collect, day-plan, checkup, machine-ready, machine-new-user, plugin-new
    title: str
    personal: bool = True  # the page may hold your own data: open it locally, never publish it
    done: list[Item] = field(default_factory=list)
    blockers: list[Item] = field(default_factory=list)
    next: list[Item] = field(default_factory=list)
    counts: dict[str, int | float] = field(default_factory=dict)
    commands: list[str] = field(default_factory=list)
    logs: list[tuple[str, str]] = field(default_factory=list)  # page only: (title, text) shown folded under Details
    verdict: str = ""  # one line at the top; empty: careerbrew's own words for the status
    headline: str = ""  # the run's key fact in counts (e.g. "corporate readiness: 7/9"): in the summary line, never business text
    failed: bool = False  # blocked even with no blocker item (a run that exited non-zero)
    at: str = ""
    id: str = ""

    @property
    def status(self) -> str:
        return "blocked" if self.blockers or self.failed else "ok"


def enabled() -> bool:
    from . import config

    try:
        return bool(config.get("run_pages"))
    except (OSError, ValueError, RuntimeError):  # an unreadable settings file must not break the run itself
        return True


def folder() -> Path:
    d = paths.sub("out", "reports", ".keep").parent
    d.mkdir(parents=True, exist_ok=True)
    return d


# ---------- rendering ----------

CSS = """
:root{color-scheme:light;--bg:#fcfcfb;--text:#0b0b0b;--muted:#52514e;--line:#e4e3df;--code-bg:#f2f1ee;
--good:#0a7a0a;--bad:#c62f2e;--accent:#2a78d6;--good-bg:#e9f5e9;--bad-bg:#fbeaea}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){color-scheme:dark;--bg:#1a1a19;--text:#f0efec;
--muted:#c3c2b7;--line:#3a3936;--code-bg:#2c2b29;--good:#5cc05c;--bad:#ee7b7a;--accent:#5a9fec;--good-bg:#1f2e1f;--bad-bg:#3a2424}}
:root[data-theme="dark"]{color-scheme:dark;--bg:#1a1a19;--text:#f0efec;--muted:#c3c2b7;--line:#3a3936;--code-bg:#2c2b29;
--good:#5cc05c;--bad:#ee7b7a;--accent:#5a9fec;--good-bg:#1f2e1f;--bad-bg:#3a2424}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:15px/1.5 -apple-system,system-ui,"Segoe UI",sans-serif}
main{max-width:860px;margin:0 auto;padding:24px 16px 48px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:28px 0 8px}
.muted{color:var(--muted)}.small{font-size:13px}
.status{display:inline-block;font-weight:600;padding:2px 10px;border-radius:999px;margin:8px 0}
.status-ok{background:var(--good-bg);color:var(--good)}.status-blocked{background:var(--bad-bg);color:var(--bad)}
.kpis{display:flex;flex-wrap:wrap;gap:8px 24px;margin:12px 0 4px;padding:0;list-style:none}
.kpis b{display:block;font-size:22px;line-height:1.2}
.items{padding-left:22px;margin:0}
.items>li{margin:0 0 12px;padding-left:4px;overflow-wrap:anywhere;font-weight:600}
.items>li>div{font-weight:400}
.commands .items>li{font-weight:400}
.count{font-size:13px;color:var(--muted);font-weight:400}
code,pre{font:13px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--code-bg);border-radius:6px}
code{padding:1px 6px;overflow-wrap:anywhere;word-break:break-word}
pre{padding:10px 12px;white-space:pre-wrap;overflow-wrap:anywhere;margin:6px 0 0}
.fix{margin-top:2px}.fix b,a{color:var(--accent)}
.blockers .items>li{color:var(--bad)}.blockers .items>li>div{color:var(--text)}
.empty{color:var(--muted);margin:0}
details{margin:6px 0;border-top:1px solid var(--line);padding-top:6px}summary{cursor:pointer}
footer{margin-top:32px;padding-top:12px;border-top:1px solid var(--line);color:var(--muted);font-size:13px}
"""


def esc(x: object) -> str:
    return html.escape(str(x))


def _item_html(it: Item, kind: str) -> str:
    parts = [esc(it.label)]
    if it.detail:
        parts.append(f'<div class="detail">{esc(it.detail)}</div>')
    if it.evidence:
        parts.append(f'<div class="muted small">{esc(it.evidence)}</div>')
    if it.why:
        parts.append(f'<div><span class="muted">Why:</span> {esc(it.why)}</div>')
    if it.command:
        label = {"done": "Ran", "blockers": "Fix", "next": "Run"}[kind]
        parts.append(f'<div class="fix"><b>{label}:</b> <code>{esc(it.command)}</code></div>')
    return "<li>" + "".join(parts) + "</li>"


def _section(title: str, kind: str, items: list[Item], empty: str) -> str:
    tag = "ol" if kind == "next" else "ul"
    body = f'<{tag} class="items">' + "".join(_item_html(it, kind) for it in items) + f"</{tag}>" if items else f'<p class="empty">{esc(empty)}</p>'
    return f'<section class="{kind}" aria-labelledby="h-{kind}"><h2 id="h-{kind}">{esc(title)} <span class="count">({len(items)})</span></h2>{body}</section>'


_ISO_DAY = re.compile(r"\b(\d{4})-(\d{2})-(\d{2})\b(?![T\d])")


def _words(text: str) -> str:
    """Days in a page heading read as words, "Plan for Mon Oct 19", like the web app (US-76); the data keeps ISO."""
    from datetime import date

    def day(m: re.Match[str]) -> str:
        try:
            d = date(int(m[1]), int(m[2]), int(m[3]))
        except ValueError:
            return m[0]
        return f"{d:%a %b} {d.day}"

    return _ISO_DAY.sub(day, text)


def _when(at: str) -> str:
    """The run's time as words, "Mon Oct 19, 09:00", in the time it was written in."""
    from datetime import datetime

    try:
        t = datetime.fromisoformat(at)
    except ValueError:
        return at
    return f"{t:%a %b} {t.day}, {t:%H:%M}"


def _fmt(v: object) -> str:
    return f"{v:g}" if isinstance(v, float) else str(v)


def render(r: Run) -> str:
    """The page as one HTML string: no script, no external file, no network."""
    verdict = r.verdict or ("Blocked: fix the blockers first" if r.status == "blocked" else "OK: nothing blocks")
    kpis = "".join(f"<li><b>{esc(_fmt(v))}</b>{esc(k.replace('_', ' '))}</li>" for k, v in r.counts.items())
    if not r.personal:
        who = "Holds no personal data (repo facts only), so it may be shared."
    elif r.id:
        who = "Holds your own data. It stays on this machine: open it with <code>careerbrew open --report latest</code>; do not publish it."
    else:
        who = "Holds the run's own output. Keep it on this machine; do not publish it."
    logs = "".join(f"<details><summary>{esc(t)}</summary><pre>{esc(text.rstrip()) or '(no output)'}</pre></details>" for t, text in r.logs)
    commands = "".join(f"<li><code>{esc(c)}</code></li>" for c in r.commands)
    return (
        '<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
        f"<title>{esc(_words(r.title))}</title><style>{CSS}</style></head><body><main>"
        f'<h1>{esc(_words(r.title))}</h1><p class="muted small">{esc(_when(r.at))}' + (f" · run <code>{esc(r.id)}</code>" if r.id else "") + "</p>"
        f'<p class="status status-{r.status}" role="status"><strong>Verdict:</strong> {esc(verdict)}</p>'
        + (f'<p class="headline">{esc(_words(r.headline))}</p>' if r.headline else "")
        + f'<p class="small jump"><a href="#h-blockers">Blockers ({len(r.blockers)})</a> · <a href="#h-next">Next ({len(r.next)})</a> · <a href="#h-done">Done ({len(r.done)})</a></p>'
        + (f'<ul class="kpis">{kpis}</ul>' if kpis else "")
        + _section("Blockers", "blockers", r.blockers, "No blockers.")
        + _section("Next", "next", r.next, "Nothing to do next.")
        + _section("Done", "done", r.done, "Nothing finished in this run.")
        + (f'<section class="commands" aria-labelledby="h-commands"><h2 id="h-commands">Commands run</h2><ul class="items">{commands}</ul></section>' if commands else "")
        + (f'<section aria-labelledby="h-details"><h2 id="h-details">Details</h2>{logs}</section>' if logs else "")
        + f"<footer><p>{who}</p><p>Agents: <code>careerbrew report latest --json</code> gives the counts, ids, statuses, and commands, with no page text. careerbrew {esc(__version__)}.</p></footer>"
        "</main></body></html>"
    )


# ---------- the content-free summary ----------


def _clean(text: str, limit: int = 300) -> str:
    return privacy.redact(" ".join(str(text).split()))[:limit]


def summary(r: Run, page: Path | None = None) -> dict[str, Any]:
    """Counts, ids, statuses, and commands: what an agent may read. `detail` and the logs never enter it."""
    keep = {"done": ("id", "label", "evidence", "command"), "blockers": ("id", "label", "why", "command"), "next": ("id", "label", "command")}
    out: dict[str, Any] = {
        "id": r.id,
        "run": r.run,
        "title": _clean(r.title),
        "at": r.at,
        "status": r.status,
        "headline": _clean(r.headline),
        "personal": r.personal,
        "page": str(page) if page else None,
        "counts": dict(r.counts),
        "commands": [_clean(c) for c in r.commands],
    }
    for kind, fields in keep.items():
        out[kind] = [{f: _clean(getattr(it, f)) for f in fields} for it in getattr(r, kind)]
    return out


# ---------- the store ----------


def _new_id(run: str, at: str, seq: int) -> str:
    digits = re.sub(r"[^0-9]", "", at[:19])  # date and time of day, without separators
    base = f"{run}-{digits[:8]}-{digits[8:14]}"
    # A second page in the same second (a pinned clock) gets its write number, which only grows: no id is ever reused.
    same = [s for s in all_summaries() if s["id"] == base or s["id"].startswith(base + "-")]
    return f"{base}-{seq}" if same or (folder() / f"{base}.html").exists() else base


def write(r: Run) -> Path | None:
    """Write the page and its summary; None when `run_pages` is off."""
    if not enabled():
        return None
    if not RUN_RE.match(r.run):
        raise ValueError(f"run name {r.run!r}: use lowercase letters, digits, and dashes")
    from .calendar import now

    # seq: the write order, so pages written in the same second (a pinned clock in tests and e2e runs) still sort.
    seq = 1 + max((int(s.get("seq", 0)) for s in all_summaries()), default=0)
    r.at = r.at or now().isoformat(timespec="seconds")
    r.id = r.id or _new_id(r.run, r.at, seq)
    d = folder()
    page = d / f"{r.id}.html"
    page.write_text(render(r), encoding="utf-8")  # out/ is plaintext by design (vault.PLAIN_DIRS)
    data = json.dumps({**summary(r, page), "seq": seq}, indent=1, sort_keys=True)
    (d / f"{r.id}.json").write_text(data, encoding="utf-8")
    shutil.copyfile(page, d / "latest.html")
    (d / "latest.json").write_text(data, encoding="utf-8")
    _prune(r.run)
    events.emit("report.write", command=r.run, status=r.status, done=len(r.done), blockers=len(r.blockers), next=len(r.next))
    return page


def _prune(run: str) -> None:
    for old in [s for s in all_summaries() if s.get("run") == run][KEEP_PER_RUN:]:
        (folder() / f"{old['id']}.json").unlink(missing_ok=True)
        (folder() / f"{old['id']}.html").unlink(missing_ok=True)


def all_summaries() -> list[dict[str, Any]]:
    """Every kept summary, newest first."""
    rows = []
    for p in folder().glob("*.json"):
        if not ID_RE.match(p.stem):
            continue
        try:
            rows.append({**json.loads(p.read_text(encoding="utf-8")), "id": p.stem})  # the id is the file name, never a field read back
        except (OSError, ValueError):
            continue
    return sorted(rows, key=lambda s: (int(s.get("seq", 0)), str(s.get("at", ""))), reverse=True)  # seq: write order


def latest(run: str | None = None) -> dict[str, Any] | None:
    rows = [s for s in all_summaries() if run is None or s.get("run") == run]
    return rows[0] if rows else None


def resolve(ref: str) -> dict[str, Any] | None:
    """A summary by id, `latest`, or a run name (its newest page). Anything else (a path) is None."""
    if ref == "latest":
        return latest()
    if ID_RE.match(ref):
        p = folder() / f"{ref}.json"
        return json.loads(p.read_text(encoding="utf-8")) if p.exists() else None
    if RUN_RE.match(ref):
        return latest(ref)
    return None


def page_html(ref: str) -> tuple[dict[str, Any], str] | None:
    s = resolve(ref)
    if not s:
        return None
    p = folder() / f"{s['id']}.html"
    return (s, p.read_text(encoding="utf-8")) if p.exists() else None


# ---------- the end of a run ----------


def summary_line(r: Run) -> str:
    first = f"; next: {r.next[0].command}" if r.next and r.next[0].command else ""
    head = f"{r.headline}; " if r.headline else ""
    return f"{r.run}: {r.status}: {head}{len(r.done)} done, {len(r.blockers)} blocker(s), {len(r.next)} next action(s){first}"


def text_lines(s: dict[str, Any]) -> list[str]:
    """A summary as plain text (`careerbrew report latest`): the page's items without their detail, one per line."""
    head = f"{s['headline']}; " if s.get("headline") else ""
    out = [f"{s['run']}: {s['status']} at {s['at']}: {head}{len(s['done'])} done, {len(s['blockers'])} blocker(s), {len(s['next'])} next action(s)"]
    out += [f"done     {d['label']}" + (f": {d['evidence']}" if d.get("evidence") else "") for d in s["done"]]
    out += [f"blocker  {b['label']}: {b['why']}" + (f" -> fix: {b['command']}" if b.get("command") else "") for b in s["blockers"]]
    out += [f"next {n}. {x['label']}" + (f" -> {x['command']}" if x.get("command") else "") for n, x in enumerate(s["next"], 1)]
    return out


def finish(r: Run, say: Callable[[str], None] | None = None) -> Path | None:
    """End a run: write its page, then print the page path and one summary line (nothing when pages are off).

    With pages on, this is all a run prints: its step lines are on the page (see `cli.run_say`), and
    `careerbrew report latest` prints the page's items as text."""
    page = write(r)
    if page is None:
        return None
    out = say or (lambda s: print(privacy.redact(s)))
    out(f"page: {page}")
    out(summary_line(r) + " (agents: careerbrew report latest --json)")
    return page


USAGE = "usage: python -m careerbrew.report enabled | html <title> <log file> <out.html> | html <title> <section dir> <out.html> <exit code>"


def html_page(title: str, source: Path, out: Path, rc: int | None = None) -> Path:
    """The one renderer behind scripts/report-html.sh.

    A log file (`== section` lines, `->` next steps, FAIL marks, a `checkup: ...` verdict) renders to `out` only.
    A section folder from scripts/checkup.sh renders the richer repo checkup: kept as a run page (when `run_pages`
    is on, with its path and one summary line printed) and copied to `out`.
    """
    from . import runs
    from .calendar import now

    out.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        r = runs.repo_checkup(source, 0 if rc is None else rc, title)
        page = finish(r)
        if page is not None:
            shutil.copyfile(page, out)
            return out
    else:
        r = runs.from_log(title, source.read_text(encoding="utf-8", errors="replace"))
    r.at = r.at or now().isoformat(timespec="seconds")
    out.write_text(render(r), encoding="utf-8")
    if source.is_dir():  # pages are off, yet the script asked for this page: say where it is
        print(privacy.redact(f"page: {out}"))
        print(privacy.redact(summary_line(r)))
    return out


def main(argv: list[str] | None = None) -> int:
    """`python -m careerbrew.report ...`: the bridge for scripts/checkup.sh and scripts/report-html.sh."""
    args = list(sys.argv[1:] if argv is None else argv)
    if args[:1] == ["enabled"]:
        return 0 if enabled() else 1
    if args[:1] == ["html"] and len(args) in (4, 5):
        rc = int(args[4]) if len(args) == 5 else None
        html_page(args[1], Path(args[2]), Path(args[3]), rc)
        return 0
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
