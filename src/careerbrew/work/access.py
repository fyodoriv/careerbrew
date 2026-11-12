"""Access blockers (issue #314): route the ask to the owning team's channel, and judge each draft's channel.

A directory file lists the support channels (`id`, `owns` topic words, `members`) and their open
`threads` (`channel`, `url`, `text`). It is read only; nothing here sends, posts, or opens anything.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

BLOCKER = re.compile(
    r"\b(permission|permissions|access|denied|forbidden|unauthori[sz]ed|not allowed|policy|role|group|entitlement)\b",
    re.I,
)
ASKS = re.compile(r"\b(need|needs|missing|request|requesting|grant|blocked|can't|cannot|no)\b", re.I)
MENTION = re.compile(r"<@([A-Z0-9]+)>|(?<![\w<])@([A-Za-z0-9][\w.-]*)")


class AccessError(ValueError):
    pass


def load(path: Path) -> dict[str, Any]:
    try:
        d = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        raise AccessError(f"could not read the directory file: {e.__class__.__name__}") from None
    if not isinstance(d, dict) or not isinstance(d.get("channels"), list):
        raise AccessError("the directory file needs a `channels` list")
    d.setdefault("threads", [])
    return d


def blocker(text: str) -> bool:
    """An access ask: names a permission, policy, role, or group, and says it is missing or needed."""
    return bool(BLOCKER.search(text) and ASKS.search(text))


def _topic(text: str, channel: dict[str, Any]) -> str:
    low = text.lower()
    hits = [t for t in channel.get("owns", []) if str(t).lower() in low]
    return max(hits, key=len) if hits else ""


def owner(text: str, directory: dict[str, Any]) -> tuple[dict[str, Any] | None, str]:
    """The channel whose owned topic the text names (the longest match wins)."""
    best: tuple[dict[str, Any] | None, str] = (None, "")
    for c in directory["channels"]:
        t = _topic(text, c)
        if len(t) > len(best[1]):
            best = (c, t)
    return best


def existing(channel: dict[str, Any], topic: str, directory: dict[str, Any]) -> dict[str, Any] | None:
    return next((t for t in directory["threads"] if t.get("channel") == channel.get("id") and topic.lower() in str(t.get("text", "")).lower()), None)


def route(text: str, directory: dict[str, Any], teammate: str = "") -> dict[str, Any]:
    """Where the ask goes. action: `none` (no access ask), `unknown` (no channel owns it), `link` (a thread exists), or `open`."""
    if not blocker(text):
        return {"action": "none", "reason": "not an access ask"}
    c, topic = owner(text, directory)
    if not c:
        return {"action": "unknown", "reason": "no channel in the directory owns this topic: add it to `owns`"}
    r: dict[str, Any] = {"channel": c["id"], "topic": topic}
    t = existing(c, topic, directory)
    if t:
        r |= {"action": "link", "thread": t.get("url", ""), "reason": "an open thread already asks this: link it, never open a second one"}
    else:
        r |= {"action": "open", "reason": "no open thread: draft the ask in the owning channel"}
    if teammate:
        r["tag"] = "in thread" if teammate in c.get("members", []) else "link from the original thread"
    return r


def mentions(text: str) -> list[str]:
    return [a or b for a, b in MENTION.findall(text)]


def judge(draft: str, channel_id: str, directory: dict[str, Any], new_thread: bool = False) -> list[str]:
    """Every reason the draft may not be saved in this channel; empty means it passes. Reasons hold ids, never draft text."""
    c = next((c for c in directory["channels"] if c.get("id") == channel_id), None)
    if not c:
        return [f"channel {channel_id} is not in the directory"]
    fails: list[str] = []
    topic = _topic(draft, c)
    if not topic:
        fails.append(f"channel {channel_id} does not own the draft's topic")
    elif new_thread and (t := existing(c, topic, directory)):
        fails.append(f"duplicates the open thread {t.get('url', '')}: link it instead")
    members = set(c.get("members", []))
    fails += [f"mentions {m}, who is not a member of {channel_id}" for m in mentions(draft) if m not in members]
    return fails
