"""Teacher mode: one step at a time toward the owner's upcoming goal.

`next_step()` returns the single most useful thing to do now, in this order:
1. an approval an agent is waiting on (it blocks work, so it comes first),
2. the prevention card of a mistake that just repeated,
3. due flashcards (spacing before new material), one card per step,
4. a lesson to confirm before the first exercise of a new pattern,
5. the next exercise for the goal's patterns,
6. a card the owner wrote that has not been validated yet,
7. a resume to send for a job the owner chose (careerbrew prepares; the owner sends),
then "done for today" when the goal's daily minutes are used.

`complete(step, result)` records the outcome (timed), applies the teaching rules in
`knowledge/teaching.yaml` (hints after failures, the solution after three, a lesson
again after "not yet"), and returns a short, polite message plus the next step.
`improvements()` turns signals (slow steps, stuck exercises, repeated mistakes, skips,
leeches, "not yet" answers) into proposals the `teacher` skill applies, so most
feedback is predicted and fixed before the owner has to write it.
"""

from __future__ import annotations

import statistics
from collections import Counter
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import yaml

from . import approvals, calendar, events, goals, paths, store
from .tutor import cards, exercises, mistakes

HINTS_BEFORE_SOLUTION = 3


# Fixes the teacher applies on its own, without waiting for anyone; the rest wait for the teacher skill.
AUTO = {
    "repeat-mistake": "applied: exercises now open with a 'watch out' tip for the mistake kinds you make most",
    "stuck-exercise": "applied: a worked example after two failures, the reference solution after three, and the exercise comes back later",
    "low-confirm": "applied: the lesson comes back with a worked example",
    "card-leech": "applied: the card is suspended so it stops costing time",
}


def rules() -> dict[str, Any]:
    return yaml.safe_load((paths.REPO_ROOT / "knowledge" / "teaching.yaml").read_text(encoding="utf-8")) or {}


def rules_version() -> str:
    return store.digest((paths.REPO_ROOT / "knowledge" / "teaching.yaml").read_text(encoding="utf-8"))[:8]


def _log_path() -> Path:
    return paths.sub("teach", "steps.jsonl")


def _state_path() -> Path:
    return paths.sub("teach", "state.json")


def _drafts_path() -> Path:
    return paths.sub("teach", "drafts.json")


def save_draft(exercise_id: str, code: str) -> None:
    """Keep typed exercise code on disk (encrypted at rest), so a restart or reload does not lose it."""
    if not exercise_id or len(code) > 100_000:
        raise ValueError("send an exercise id and at most 100000 characters of code")
    store.write_json(_drafts_path(), {**drafts(), exercise_id: code})


def drafts() -> dict[str, str]:
    return store.read_json(_drafts_path(), {}) or {}


def log() -> list[dict[str, Any]]:
    return store.read_jsonl(_log_path())


def _today_rows() -> list[dict[str, Any]]:
    """Today's log rows. Rows are stamped in UTC; count each by its day in the settings zone, the zone of `calendar.now()`.

    The machine's own zone plays no part: today and each row's day must come from one zone, or a step late in the
    evening (already tomorrow in UTC, or on the machine) counts on no day.
    """
    tz = calendar.local_tz()
    today = calendar.now().astimezone(tz).date()
    return [r for r in log() if datetime.fromisoformat(r["at"]).astimezone(tz).date() == today]


def state() -> dict[str, Any]:
    return store.read_json(_state_path(), {}) or {}


def _save_state(s: dict[str, Any]) -> None:
    store.write_json(_state_path(), s)


def _today_minutes() -> int:
    return sum(int(r.get("seconds") or 0) for r in _today_rows()) // 60


def _confirmed_patterns() -> set[str]:
    return set(state().get("confirmed_patterns", []))


def _validated_cards() -> set[str]:
    return set(state().get("validated_cards", []))


def _sent_jobs() -> set[str]:
    from .jobs import strategy

    applied = {r.get("posting_id") or r.get("company", "").casefold() for r in store.read_jsonl(strategy.applications_path())}
    return applied | set(state().get("skipped_jobs", []))


def _step(kind: str, title: str, why: str, **data: Any) -> dict[str, Any]:
    # The step timer runs on the wall clock: it measures real seconds, and a pinned clock never moves.
    return {"kind": kind, "title": title, "why": why, "started": datetime.now(UTC).isoformat(timespec="seconds"), **data}


def next_step() -> dict[str, Any]:
    goal = goals.load()["upcoming"]
    focus = goals.focus(goal)
    budget = int((goal or {}).get("minutes_per_day") or goals.DEFAULT_MINUTES)
    for a in approvals.pending()[:1]:
        return _step("approve", f"Approve or reject: {a['title']}", "An agent is waiting for your decision.", approval=a)
    if not goal:
        # #54: no goal means no measured reason to study; lessons wait until the owner picks one.
        return _step("goal", "Set a goal first", "Lessons and exercises follow your upcoming goal, and none is set yet.", setup='careerbrew goals set "<your goal>"')
    recent = mistakes.load()[-5:]
    rep = next((m for m in reversed(recent) if m["repeat"] and m["at"] > state().get("last_alarm", "")), None)
    if rep:
        return _step("mistake", f"This mistake repeated: {rep['kind'].replace('-', ' ')} in {rep['pattern']}", "Repeated mistakes get fixed first, so they stop.", mistake=rep)
    if _today_minutes() >= budget:
        return _step("done", "That is today's study time. Well done.", f"You used your {budget} minutes for {goal['title'] if goal else 'today'}.", summary=summary())
    due = cards.queue(limit=1, topics=focus.get("card_topics"))
    low = _low_rated_kinds()
    if (back := _retry_due()) is not None:  # the exercise the solution was shown for comes back, as promised (issue #185)
        return back
    if due and due[0]["mode"] == "review" and not ("card" in low and "exercise" not in low):
        c = due[0]
        return _step("card", c["question"], "Due cards come before new material, so you keep what you learned.", card=c)
    ex = exercises.next_exercise(focus["patterns"], skip=set(state().get("retry_later", []))) if "exercise" in focus["activities"] or not goal else None
    if ex and ex["pattern"] not in _confirmed_patterns():
        extra: dict[str, Any] = {}
        if any(r["kind"] == "lesson" and r["outcome"] == "not-yet" and r["ref"] == ex["pattern"] for r in log()):
            # low-confirm fix, applied automatically: the second time, the lesson comes with a tiny worked example
            first = next(e for e in exercises.load() if e["pattern"] == ex["pattern"])
            extra["example"] = {"title": first["title"], "statement": first["statement"], "solution": first["solution"]}
        return _step(
            "lesson",
            f"Lesson: {ex['pattern']}",
            f"You are about to start {ex['pattern']} exercises; learn the idea first.",
            pattern=ex["pattern"],
            lesson=ex["lesson"],
            prompt=f"In one sentence, when would you use {ex['pattern']}?",
            **extra,
        )
    if ex:
        tries = [a for a in exercises.attempts(ex["id"]) if not a["correct"]]
        public = {k: ex[k] for k in ("id", "order", "pattern", "difficulty", "title", "statement", "signature", "lesson")}
        public["starter"] = drafts().get(ex["id"]) or ex.get("starter") or f"{ex['signature']}\n    pass\n"
        public["hints"] = ex["hints"][: min(len(tries), len(ex["hints"]))]
        if len(tries) >= 2:  # worked-example rule: study a solved example from the same pattern before the third try
            done = exercises.status()
            prior = [e for e in exercises.load() if e["pattern"] == ex["pattern"] and e["order"] < ex["order"] and done.get(e["id"], {}).get("solved")]
            if prior:
                public["worked_example"] = {"title": prior[-1]["title"], "statement": prior[-1]["statement"], "solution": prior[-1]["solution"]}
        public["watch_out"] = watch_out(ex)
        public["tests"] = [{"args": t.get("args", t.get("ops")), "expected": t["expected"], "note": t.get("note", "")} for t in ex["tests"][:2]]
        return _step("exercise", ex["title"], f"Next in the curriculum for {focus['why']}: {ex['pattern']}, {ex['difficulty']}.", exercise=public, attempts=len(tries))
    if due:
        c = due[0]
        return _step("card", c["question"], "A due card." if c["mode"] == "review" else "A new card for your goal.", card=c)
    mine = [c for c in cards.all_cards().values() if c["deck"] in ("mine", "notion") and c["id"] not in _validated_cards()]
    if mine:
        c = cards.with_explanation(mine[0])
        return _step("validate", f"Check your card: {c['question']}", "Cards you wrote are checked once against the explanation, so you never memorise a wrong answer.", card=c)
    from .jobs import strategy

    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    for tier in ("practice", "target", "dream"):
        for row in (plan.get("tiers") or {}).get(tier, []):
            if row["posting_id"] not in _sent_jobs() and row["company"].casefold() not in _sent_jobs():
                return _step(
                    "resume",
                    f"Send your resume to {row['company']}",
                    f"You chose this {tier} job. careerbrew never sends anything: open the posting, send it yourself, then confirm.",
                    job=row,
                    checklist=[
                        "Open the posting and re-read the requirements.",
                        "Tailor the top three resume bullets to the role's skills: " + ", ".join(row.get("skills", [])[:5]),
                        "Send it from your own email or the company's form.",
                        "Come back and press 'I sent it'.",
                    ],
                )
    _ = strategy
    return _step("done", "Nothing is waiting. You are ahead of your plan.", "Set a new upcoming goal, choose more jobs, or add cards.", summary=summary())


RETRY_DAYS = 3  # an exercise the reference solution was shown for comes back after this many days
LOW_RATING, LOW_TIMES, LOW_DAYS = 2, 2, 30  # this many feedback ratings of LOW_RATING or less on one step kind move it later


def _retry_due() -> dict[str, Any] | None:
    """The first exercise on the retry list whose last failed try is RETRY_DAYS old and that is still unsolved."""
    # Reads only: the view refetches this, so the retry leaves the list when it is tried (`complete`), not when shown.
    solved = {k for k, v in exercises.status().items() if v.get("solved")}
    keep = [i for i in state().get("retry_later", []) if i not in solved]
    cut = (calendar.now_utc() - timedelta(days=RETRY_DAYS)).isoformat()
    for ex_id in keep:
        tries = exercises.attempts(ex_id)
        if tries and str(tries[-1].get("at", "")) <= cut:
            ex = exercises.get(ex_id)
            public = {k: ex[k] for k in ("id", "order", "pattern", "difficulty", "title", "statement", "signature", "lesson")}
            public["starter"] = drafts().get(ex_id) or ex.get("starter") or f"{ex['signature']}\n    pass\n"
            public["hints"], public["watch_out"] = [], watch_out(ex)
            public["tests"] = [{"args": t.get("args", t.get("ops")), "expected": t["expected"], "note": t.get("note", "")} for t in ex["tests"][:2]]
            return _step("exercise", ex["title"], f"Back after {RETRY_DAYS} days, as promised: try it again from memory.", exercise=public, attempts=0, retry=True)
    return None


def _low_rated_kinds() -> set[str]:
    """Step kinds the owner rated LOW_RATING or less at least LOW_TIMES times in LOW_DAYS days: they move later."""
    cut = (calendar.now_utc() - timedelta(days=LOW_DAYS)).isoformat(timespec="seconds")
    counts: dict[str, int] = {}
    for r in store.read_jsonl(_feedback_path()):
        if r.get("step") and isinstance(r.get("rating"), int) and r["rating"] <= LOW_RATING and str(r.get("at", "")) >= cut:
            counts[r["step"]] = counts.get(r["step"], 0) + 1
    return {k for k, n in counts.items() if n >= LOW_TIMES}


def watch_out(ex: dict[str, Any]) -> list[str]:
    """Predicted mistakes, prevented up front: this exercise's pitfall tips for the mistake kinds you make most."""
    kinds = Counter("timeout" if m["kind"] == "complexity" else m["kind"] for m in mistakes.load() if not m["source"].startswith("lesson:"))
    pit = ex.get("pitfalls") or {}
    return [pit[k] for k, _ in kinds.most_common() if pit.get(k)][:2]


def complete(step: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
    """Record a finished step and return the teacher's message and the next step."""
    kind = step["kind"]
    # Wall clock, as in `_step`: the real seconds the owner spent on this step.
    started = datetime.fromisoformat(step["started"]) if step.get("started") else datetime.now(UTC)
    seconds = max(0, int((datetime.now(UTC) - started).total_seconds()))
    s = state()
    message, outcome = "", "done"
    if kind == "approve":
        a = approvals.decide(step["approval"]["id"], result["decision"], result.get("note", ""))
        message, outcome = f"Recorded: {a['status']}. The agent will see it.", a["status"]
    elif kind == "mistake":
        s["last_alarm"] = step["mistake"]["at"]
        message = f"Good. Remember: {step['mistake']['tip']}"
    elif kind == "card":
        r = cards.review(step["card"]["id"], result["rating"], seconds=seconds)
        outcome = result["rating"]
        message = "Leech: rewrite this card smaller." if r["leech"] else ("Keep going." if result["rating"] in ("good", "easy") else "It will come back soon. Read the explanation once more.")
    elif kind == "lesson":
        if result.get("confirmed"):
            s["confirmed_patterns"] = sorted({*s.get("confirmed_patterns", []), step["pattern"]})
            message, outcome = "Confirmed. Now try it on a real problem.", "confirmed"
        else:
            mistakes.record(f"lesson:{step['pattern']}", step["pattern"], "concept", "not yet confident after the lesson")
            message, outcome = "That is fine. Read it once more, then explain it in your own words; it will come back tomorrow.", "not-yet"
    elif kind == "exercise":
        if step.get("retry") and step["exercise"]["id"] in s.get("retry_later", []):  # back in the curriculum; three more fails send it again
            s.setdefault("retry_since", {})[step["exercise"]["id"]] = calendar.now_utc().isoformat(timespec="seconds")
            s["retry_later"] = [i for i in s["retry_later"] if i != step["exercise"]["id"]]
        r = exercises.submit(step["exercise"]["id"], result["code"], seconds=seconds, hints_used=len(step["exercise"].get("hints", [])))
        g = r["grade"]
        outcome = "pass" if g["correct"] else "fail"
        since = str(state().get("retry_since", {}).get(step["exercise"]["id"], ""))  # fails since it came back (issue #185)
        fails = len([a for a in exercises.attempts(step["exercise"]["id"]) if not a["correct"] and str(a.get("at", "")) > since])
        if g["correct"]:
            cx = g["complexity"]
            message = (
                "All tests pass. Now say in one sentence why it is correct and what its complexity is."
                + (f" Complexity {cx['expected']}: ok." if cx and cx["ok"] else " It works but grows too fast: aim for " + cx["expected"] + "." if cx else "")
                + (" Clean code." if not g["style"] else " Tidy: " + ", ".join(g["style"]) + ".")
            )
        elif fails >= HINTS_BEFORE_SOLUTION:
            message = "Three tries: here is the reference solution. Read it line by line; this exercise comes back in a few days."
            r["solution"] = exercises.get(step["exercise"]["id"])["solution"]
            if step["exercise"]["id"] not in s.setdefault("retry_later", []):
                s["retry_later"].append(step["exercise"]["id"])
        else:
            m = r["mistakes"][0] if r["mistakes"] else None
            message = (f"{'Again: ' if m and m['repeat'] else ''}{m['tip']}" if m else "Not yet.") + " A new hint is unlocked."
        _save_state(s)
        _record(kind, outcome, seconds, step)
        return {"message": message, "result": r, "next": next_step()}
    elif kind == "validate":
        decision = result.get("decision", "keep")
        if decision == "fix" and result.get("answer"):
            cards.add(step["card"]["question"], result["answer"], step["card"].get("tags", []), deck=step["card"]["deck"])
        s["validated_cards"] = sorted({*s.get("validated_cards", []), step["card"]["id"]})
        message, outcome = {"keep": "Validated.", "fix": "Fixed and validated.", "drop": "Dropped from your review."}.get(decision, "Validated."), decision
        if decision == "drop":
            s.setdefault("dropped_cards", []).append(step["card"]["id"])
            try:
                cards.suspend(step["card"]["id"])  # out of review too, not only out of the check (issue #185)
            except KeyError:
                pass  # the card left its deck meanwhile: nothing to suspend
    elif kind == "resume":
        from .jobs import strategy

        if result.get("sent"):
            strategy.log_application(step["job"]["company"], "applied", "applied")
            message, outcome = f"Logged: applied to {step['job']['company']}. Good luck.", "sent"
        else:
            s["skipped_jobs"] = sorted({*s.get("skipped_jobs", []), step["job"]["posting_id"]})
            message, outcome = "Skipped for now.", "skipped"
    _save_state(s)
    _record(kind, outcome, seconds, step)
    return {"message": message, "next": next_step()}


def _record(kind: str, outcome: str, seconds: int, step: dict[str, Any]) -> None:
    ref = (step.get("exercise") or {}).get("id") or (step.get("card") or {}).get("id") or step.get("pattern") or ""
    store.append_jsonl(_log_path(), [{"at": calendar.now_utc().isoformat(timespec="seconds"), "kind": kind, "outcome": outcome, "seconds": seconds, "ref": ref, "rules": rules_version()}])
    events.emit("teach.step", status=outcome, seconds=seconds)


def summary() -> dict[str, Any]:
    rows = _today_rows()
    return {"steps": len(rows), "minutes": sum(int(r["seconds"]) for r in rows) // 60, "by_kind": {k: sum(1 for r in rows if r["kind"] == k) for k in {r["kind"] for r in rows}}, "mistake_repeat_rate": mistakes.repeat_rate()}


def _feedback_path() -> Path:
    return paths.sub("teach", "feedback.jsonl")


def feedback(text: str, step_kind: str = "", rating: int | None = None) -> dict[str, Any]:
    row = {"at": calendar.now_utc().isoformat(timespec="seconds"), "text": " ".join(text.split())[:2000], "step": step_kind[:30], "rating": rating}
    store.append_jsonl(_feedback_path(), [row])
    events.emit("teach.feedback", status="written")
    return row


def improvements() -> list[dict[str, Any]]:
    """Predicted feedback: what the owner would probably complain about, with a proposed fix."""
    rows = log()
    out: list[dict[str, Any]] = []
    by_kind: dict[str, list[int]] = {}
    for r in rows:
        by_kind.setdefault(r["kind"], []).append(int(r["seconds"]))
    for r in rows[-50:]:
        med = statistics.median(by_kind[r["kind"]]) if len(by_kind[r["kind"]]) >= 5 else None
        if med and r["seconds"] > 3 * med and r["seconds"] > 120:
            out.append({"signal": "slow-step", "ref": r["ref"], "proposal": f"{r['kind']} step on {r['ref'] or 'this item'} took {r['seconds'] // 60} min (median {int(med) // 60} min): split it or add a lesson before it."})
    stuck: dict[str, int] = {}
    for a in exercises.attempts():
        stuck[a["exercise"]] = 0 if a["correct"] else stuck.get(a["exercise"], 0) + 1
    out += [{"signal": "stuck-exercise", "ref": k, "proposal": f"{k}: {n} failed attempts. Add an easier warm-up exercise in its pattern, or a worked example in its lesson."} for k, n in stuck.items() if n >= 3]
    reps: dict[str, int] = {}
    for m in mistakes.load():
        if m["repeat"]:
            reps[f"{m['pattern']}: {m['kind']}"] = reps.get(f"{m['pattern']}: {m['kind']}", 0) + 1
    out += [{"signal": "repeat-mistake", "ref": k, "proposal": f"{k} repeated {n} times. Add a pitfall tip for this kind to the pattern's exercises and a focused card."} for k, n in reps.items() if n >= 2]
    kinds = [r["kind"] for r in rows[-6:] if r["outcome"] == "skipped"]
    if len(kinds) >= 2 and len(set(kinds)) == 1:
        out.append({"signal": "skipped", "ref": kinds[0], "proposal": f"{kinds[0]} steps were skipped twice in a row: move them later in the order, or ask why."})
    nots = [r for r in rows if r["kind"] == "lesson" and r["outcome"] == "not-yet"]
    out += [{"signal": "low-confirm", "ref": r["ref"], "proposal": f"The lesson for {r['ref']} did not land: rewrite it with an analogy and a tiny example."} for r in nots[-3:]]
    out += [{"signal": "card-leech", "ref": c, "proposal": f"Card {c} is a leech: rewrite it smaller, or report it for a better explanation."} for c in cards.stats()["suspended"]]
    written = store.read_jsonl(_feedback_path())
    out += [{"signal": "written", "ref": f.get("step") or "", "proposal": f"You wrote: {f['text']}"} for f in written[-10:]]
    for o in out:
        o["auto"] = AUTO.get(o["signal"], "")
    return out
