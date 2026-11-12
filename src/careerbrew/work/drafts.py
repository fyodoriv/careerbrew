"""Reply drafts that fit a thread's latest state, saved safely (issue #269, US-98).

Content rules (`fit`):
1. Read the thread to its newest message. A draft written against an older message (`seen`) is
   stale: the result counts the newer messages, and the rules below judge the newest state.
2. After a release or deploy has started, approve or acknowledge. Ask for a change only when the
   caller names a major issue (`major`), and then say why in one line.
3. A small acknowledgement (a one-word thanks, "ok", "done") gets an emoji suggestion, not a text draft.

Save rules (`replace`):
4. One helper writes every draft: clear the target's old draft, check it is gone, save the new
   text, read it back, and fail when the read-back differs (`DraftMismatch`). A chat tool that keeps
   the old text silently fails here instead of posting the wrong reply.
5. A per-target lock, so two steps never write one target at the same time. A work-day step that
   writes drafts declares `uses: slack-draft:<target>` (`uses_entry`), so the runner keeps two such
   steps in different stages (issue #258); the lock holds even when they are not.
6. A project session's claim (issue #274, `work/sessions.py`): `replace(..., session=<id>)` takes or renews the
   claim on `slack-draft:<target>` first; a target another session claimed raises `DraftClaimed`, for a writer
   with no session too (the live cycle skips it), so two sessions never write one draft.

The thread and the draft text are business data: they are never logged, and `fit` echoes only text
careerbrew wrote itself (the ack) or the caller's one-line reason. The local backend keeps each
target's draft in `<data root>/drafts/<digest>.txt`, encrypted at rest like other data files.

Lifecycle: nothing is kept in process memory; the lock is a per-call `fcntl` lock on a file next to
the draft, released when the call returns.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import re
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Protocol

from .. import events, paths, store

TARGET = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/#@-]{0,127}$")
MAX_REASON = 200
LOCK_WAIT_SECONDS = 10.0
DEFAULT_ACK = "Looks good, approved. Thanks for shipping this."

# A plan or a denial before the match ("we will deploy", "not deployed yet") means it has not started.
_FUTURE = re.compile(
    r"\b(will|tomorrow|later|tonight|planning|plan to|scheduled?|going to|before|once|when|should|shall|can we|could we|ready to|about to|"
    r"not|never|yet to|isn't|hasn't|haven't|didn't|wasn't|won't)\b",
    re.I,
)
_QUESTION = re.compile(r"^(did|has|have|is|are|was|were|can|could|should|when|will|shall|do|does|any|anyone)\b", re.I)
_STARTED = (
    re.compile(r"\b(deploying|releasing|rolling out|rolling it out|pushing to prod(uction)?|shipping it)\b", re.I),
    re.compile(r"\b(deploy|deployment|release|rollout|roll-out)\b[^\n]{0,40}?\b(start(ed|ing)?|began|begun|kicked off|in progress|underway|running|is out|went out|live|done|finished|completed?)\b", re.I),
    re.compile(r"\b(start(ed|ing)?|began|kicked off|triggered)\b[^\n]{0,30}?\b(deploy|deployment|release|rollout)", re.I),
    re.compile(r"\b(deployed|released|shipped)\b", re.I),
)
_STOPPED = re.compile(r"\b(rolled back|rolling back|rollback|reverted|reverting|aborted|halted|cancell?ed|paused|stopped|(deploy(ment)?|release|rollout) failed)\b", re.I)
_CHANGE = re.compile(
    r"\b(could you|can you|would you|please (change|update|fix|rename|add|remove|move|revert)|should (we|be|use|have)|instead of|nit|"
    r"before (we|you|this|it) (merge|ship|deploy|release)|let'?s (change|hold|wait)|request(ing)? changes|needs? (a |to )?(change|fix))\b",
    re.I,
)
_APPROVE = re.compile(r"\b(lgtm|approved?|ship it|looks good|go ahead|nice work|thanks|thank you|great work)\b|:shipit:", re.I)
# A small acknowledgement, as words left after punctuation, emoji, and mentions are gone; each maps to an emoji name.
_SMALL = {
    "pray": ("thanks", "thank you", "thx", "ty", "tysm", "thanks a lot", "thank you so much", "much appreciated", "cheers"),
    "+1": ("+1", "ok", "okay", "k", "kk", "got it", "will do", "sounds good", "sg", "sure", "noted", "ack", "roger", "yes", "yep"),
    "white_check_mark": ("done", "fixed", "merged", "lgtm", "approved", "shipped", "resolved"),
    "raised_hands": ("great", "nice", "cool", "perfect", "awesome", "amazing", "great work", "nice work", "well done"),
}


class DraftError(ValueError):
    pass


class DraftMismatch(DraftError):
    """The read-back differs from the text saved: the target holds something else."""


class DraftBusy(DraftError):
    """Another step holds the target's lock."""


class DraftClaimed(DraftBusy):
    """Another project session claimed the target (issue #274): skip it."""


class Backend(Protocol):
    """Where a target's draft lives. A company plugin wraps its chat tool in these three calls."""

    def clear(self, target: str) -> None: ...

    def save(self, target: str, text: str) -> None: ...

    def read(self, target: str) -> str | None: ...


def check_target(target: str) -> str:
    target = str(target).strip()
    if not TARGET.match(target) or ".." in target:
        raise DraftError(f"draft target {target[:40]!r}: use a short id such as C123/1700000000.000100 (letters, digits, . _ : / # @ -)")
    return target


def uses_entry(target: str) -> str:
    """The `uses:` value a work-day step declares when it writes this target's draft (issue #258)."""
    return f"slack-draft:{check_target(target)}"


def _digest(target: str) -> str:
    return hashlib.sha256(target.encode("utf-8")).hexdigest()[:24]


class LocalBackend:
    """Drafts in the data root, one encrypted file per target (the file name is a digest, never the target)."""

    def __init__(self, root: Path | None = None):
        self.root = root

    def path(self, target: str) -> Path:
        return (self.root / f"{_digest(target)}.txt") if self.root else paths.sub("drafts", f"{_digest(target)}.txt")

    def clear(self, target: str) -> None:
        self.path(target).unlink(missing_ok=True)

    def save(self, target: str, text: str) -> None:
        p = self.path(target)
        p.parent.mkdir(parents=True, exist_ok=True)
        store.write_text(p, text)

    def read(self, target: str) -> str | None:
        p = self.path(target)
        return store.read_text(p) if p.exists() else None


def normalize(text: str | None) -> str:
    """What two drafts must share to count as the same: line endings and trailing spaces do not matter."""
    lines = (text or "").replace("\r\n", "\n").replace("\r", "\n").split("\n")
    return "\n".join(line.rstrip() for line in lines).strip("\n")


def compare(sent: str, read_back: str | None) -> dict[str, Any]:
    """Content-free result of a read-back: same or not, and the first line that differs."""
    a, b = normalize(sent), normalize(read_back)
    if a == b:
        return {"same": True, "line": 0, "sent_chars": len(a), "read_chars": len(b)}
    al, bl = a.split("\n"), b.split("\n")
    line = next((i for i, (x, y) in enumerate(zip(al, bl, strict=False), 1) if x != y), min(len(al), len(bl)) + 1)
    return {"same": False, "line": line, "sent_chars": len(a), "read_chars": len(b)}


@contextmanager
def locked(target: str, wait: float = LOCK_WAIT_SECONDS, root: Path | None = None) -> Iterator[None]:
    """The target's exclusive lock; waits up to `wait` seconds, then refuses with the `uses:` fix."""
    lock = (root / f".{_digest(target)}.lock") if root else paths.sub("drafts", f".{_digest(target)}.lock")
    lock.parent.mkdir(parents=True, exist_ok=True)
    with lock.open("a") as fh:
        deadline = time.monotonic() + max(wait, 0.0)
        while True:
            try:
                fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise DraftBusy(f"another step is writing this draft; declare `uses: {uses_entry(target)}` on both steps so they run in different stages") from None
                time.sleep(0.05)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def _claim(target: str, session: str) -> None:
    """A project session's claim (issue #274): `session` takes or renews it; any other writer skips a claimed target."""
    from . import sessions

    res = uses_entry(target)
    try:
        if session:
            sessions.take(res, session)
            return
        held = sessions.holder(res)
    except sessions.ClaimHeld as e:
        raise DraftClaimed(str(e)) from None
    except sessions.SessionError as e:
        raise DraftError(str(e)) from None
    if held:
        raise DraftClaimed(f"{sessions.shown(res)} is claimed by session {held['session']} until {held['until'][11:16]}: skip it, or pass that session's --session")


def replace(target: str, text: str, backend: Backend | None = None, wait: float = LOCK_WAIT_SECONDS, if_empty: bool = False, session: str = "") -> dict[str, Any]:
    """Clear, save, read back, compare, under the target's lock. Raises `DraftMismatch` when the target keeps other text.
    `if_empty`: keep a draft the target already holds (`kept: true`), checked under the same lock (the live cycle, issue #266).
    `session`: the project session that writes it claims the target first; a target another session claimed raises `DraftClaimed`."""
    target = check_target(target)
    if not normalize(text):
        raise DraftError("the draft is empty: write the reply first (an emoji suggestion is not saved as a draft)")
    try:
        _claim(target, session)
    except DraftClaimed:
        events.emit("draft.replace", status="claimed")
        raise
    be = backend or LocalBackend()
    root = be.root if isinstance(be, LocalBackend) else None
    location = "local draft store" if isinstance(be, LocalBackend) else "draft target"
    try:
        with locked(target, wait, root):
            if if_empty and normalize(be.read(target)):
                events.emit("draft.replace", status="kept")
                return {"target": target, "kept": True}
            be.clear(target)
            left = be.read(target)
            if normalize(left):
                events.emit("draft.replace", status="not_cleared")
                raise DraftMismatch(f"the old draft is still there after clearing it; nothing was saved (fix: check the {location} before retrying)")
            be.save(target, text)
            result = compare(text, be.read(target))
    except DraftBusy:
        events.emit("draft.replace", status="busy")
        raise
    if not result["same"]:
        events.emit("draft.replace", status="mismatch", line=result["line"])
        raise DraftMismatch(f"the saved draft differs from the text you wrote at line {result['line']} ({result['read_chars']} vs {result['sent_chars']} characters); check the {location} before anyone sends it")
    events.emit("draft.replace", status="ok", chars=result["sent_chars"])
    return {"target": target, "kept": False, **result}


def _words(text: str) -> str:
    text = re.sub(r"<[^>]*>|:[a-z0-9_+-]+:|@\w+", " ", text.lower())
    text = re.sub(r"[^a-z0-9+' ]+", " ", text)
    return " ".join(text.split())


def small_ack(text: str) -> str | None:
    """The emoji name for a small acknowledgement, else None. A message of emoji alone counts as one too."""
    words = _words(text)
    if not words:
        return "+1" if text.strip() else None
    if len(words.split()) > 4:
        return None
    words = re.sub(r"^(ok|okay|great|cool|perfect|awesome) (thanks|thank you|thx|ty)$", r"\2", words)
    words = re.sub(r" (all|team|everyone|guys|folks|so much|again|man|mate)$", "", words)
    return next((emoji for emoji, phrases in _SMALL.items() if words in phrases), None)


def _sentences(text: str) -> list[str]:
    return [s for s in re.split(r"(?<=[.!?])\s+|\n+", text) if s.strip()]


def _asks(sentence: str) -> bool:
    """A question about the state ("did the deploy start?"), not a statement that ends with one."""
    return sentence.rstrip().endswith("?") and bool(_QUESTION.match(sentence.strip()))


def deploy_started(text: str) -> bool:
    """True when a message says a deploy or release has started (not planned, not asked about).

    A future word ("will", "tomorrow", "about to") before or inside the match makes it a plan;
    after it ("Deploy started, will update when done") it does not.
    """
    for s in _sentences(text):
        if _asks(s):
            continue
        for p in _STARTED:
            if (m := p.search(s)) and not _FUTURE.search(s[: m.end()]):
                return True
    return False


def deploy_stopped(text: str) -> bool:
    return any(_STOPPED.search(s) and not _asks(s) for s in _sentences(text))


def _ts_key(ts: Any) -> tuple[int, float, str]:
    try:
        return (0, float(ts), "")
    except (TypeError, ValueError):
        return (1, 0.0, str(ts))


def messages(thread: Any) -> list[dict[str, Any]]:
    """A thread as a list of {ts, text, mine}, oldest first. Takes {"messages": [...]} or a bare list."""
    raw = thread.get("messages") if isinstance(thread, dict) else thread
    if not isinstance(raw, list) or not raw:
        raise DraftError('the thread has no messages: pass {"messages": [{"ts": ..., "text": ..., "mine": false}, ...]}')
    out = []
    for n, m in enumerate(raw):
        if not isinstance(m, dict) or not isinstance(m.get("text", ""), str):
            raise DraftError(f"message {n + 1}: use an object with text, ts, and mine")
        out.append({"ts": m.get("ts", n), "text": m.get("text", ""), "mine": bool(m.get("mine") or m.get("author") == "me"), "n": n})
    if all("ts" in m for m in raw):
        out.sort(key=lambda m: (_ts_key(m["ts"]), m["n"]))
    return out


def load_thread(path: Path) -> list[dict[str, Any]]:
    try:
        return messages(json.loads(path.read_text(encoding="utf-8")))
    except (OSError, json.JSONDecodeError) as e:
        raise DraftError(f"could not read the thread file: {e.__class__.__name__}") from None


def fit(thread: list[dict[str, Any]], draft: str | None = None, seen: Any = None, major: str | None = None) -> dict[str, Any]:
    """What the reply should be, judged on the newest state of the thread.

    kind: `none` (your message is the newest), `emoji` (a reaction, not a text), `ack` (approve or
    acknowledge; `text` is careerbrew's ack unless your draft already approves), `stop` (a release
    started and you named a major issue; `text` holds the one-line reason), or `reply` (write or keep
    your own reply). `newer` counts the messages after `seen`, the newest one the draft was written for.
    """
    if major is not None:
        major = " ".join(str(major).split())
        if not major or len(major) > MAX_REASON:
            raise DraftError(f"--major: say why in one line (1 to {MAX_REASON} characters)")
    newest = thread[-1]
    newer = 0 if seen is None else sum(1 for m in thread if _ts_key(m["ts"]) > _ts_key(seen))
    out: dict[str, Any] = {"kind": "reply", "emoji": None, "text": None, "keep_draft": draft is not None, "newest_ts": newest["ts"], "newer": newer, "stale": newer > 0}
    started = False
    for m in thread:  # the latest word wins: a rollback after the start ends it, a new start after that counts again
        if deploy_stopped(m["text"]):
            started = False
        elif deploy_started(m["text"]):
            started = True
    out["deploy_started"] = started
    change = bool(draft and (_CHANGE.search(draft) or draft.strip().endswith("?")))
    if newest["mine"]:
        out.update(kind="none", keep_draft=False, reason="your message is the newest; wait for a reply")
    elif (emoji := small_ack(newest["text"])) and not (started and major):
        out.update(kind="emoji", emoji=emoji, keep_draft=False, reason="the newest message is a small acknowledgement: react, do not reply")
    elif started and major:
        out.update(kind="stop", text=f"Please hold the release: {major}", keep_draft=False, reason="a release has started and you named a major issue")
    elif started:
        keep = bool(draft and _APPROVE.search(draft) and not change)
        out.update(
            kind="ack",
            text=None if keep else DEFAULT_ACK,
            keep_draft=keep,
            reason="a release has started: approve or acknowledge" + ("; your draft asked for changes, so it was replaced" if change else ""),
        )
    elif draft is not None and (emoji := small_ack(draft)):
        out.update(kind="emoji", emoji=emoji, keep_draft=False, reason="your draft is a small acknowledgement: react instead")
    else:
        out["reason"] = "write the reply to the newest message" if draft is None else "your draft fits; keep it"
    if out["stale"] and out["kind"] == "reply" and draft is not None:
        out.update(keep_draft=False, reason=f"{newer} newer message(s) since your draft: re-read them and redraft")
    events.emit("draft.fit", status=out["kind"], stale=out["stale"], deploy_started=started)
    return out
