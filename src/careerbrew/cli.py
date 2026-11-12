"""careerbrew command line. Run `careerbrew --help`.

Output rule: commands print aggregates, ids, and file paths. Business content
(evidence titles, the promo doc text) prints only with `--show`, and only to an
interactive terminal, unless a plugin policy sets `agent_may_read_business:
true`. Reason: an agent's transcript goes to its model provider, so a piped
stdout is a data exit. Every printed line passes through privacy.redact.
"""

from __future__ import annotations

import argparse
import contextlib
import getpass
import io
import json
import os
import re
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from . import __version__, backup, connectors, events, machine, paths, plugins, privacy, review, secrets, store, sync, vault
from .charts import Report
from .jobs import criteria, sources, strategy, want
from .tutor import engine as tutor
from .work import evidence, linkedin, priorities, promo

EXAMPLES = paths.REPO_ROOT / "profile" / "examples"


def say(text: str = "") -> None:
    print(privacy.redact(text))


def _quiet(_text: str = "") -> None:
    return None


def run_say() -> Callable[[str], None]:
    """Where a long run's step lines go: stdout when run pages are off. With pages on (the default) the run's
    page holds them instead, and the run prints only the page path and one summary line (`report.finish`)."""
    from . import report

    return _quiet if report.enabled() else say


def _owner_ui() -> bool:
    from . import actions

    return actions.OWNER_UI.get()


def may_show_business() -> bool:
    return sys.stdout.isatty() or _owner_ui() or bool(plugins.effective_policy().get("agent_may_read_business"))


# ---------- init ----------
def cmd_init(_a: argparse.Namespace) -> int:
    root = paths.data_root()
    created = []
    corporate = machine.role() == "corporate"
    for src in sorted(EXAMPLES.rglob("*")):
        if corporate and src.relative_to(EXAMPLES).parts[0] in machine.PERSONAL_DIRS:
            continue
        if src.is_file():
            dest = root / src.relative_to(EXAMPLES)
            if not dest.exists():
                dest.parent.mkdir(parents=True, exist_ok=True)
                text = src.read_text(encoding="utf-8")
                if corporate and src.name == "profile.yaml":
                    text = machine.work_profile(text)
                store.write_text(dest, text)
                created.append(str(dest.relative_to(root)))
    say(f"data root: {root}")
    say(f"created {len(created)} file(s): {', '.join(created) or 'none (all present)'}")
    if corporate:
        say("next: set name and employer with `careerbrew data set profile.yaml --name '<name>' --employer '<employer>'`, then follow docs/corporate-machine.md")
    else:
        say("next: `careerbrew data edit profile.yaml` (also jobs/criteria.yaml, jobs/sources.yaml), then `careerbrew jobs scan`")
    return 0


# ---------- jobs ----------
def _scored() -> list[tuple[float, sources.Posting, dict[str, float]]]:
    prof, crit = want.profile(), criteria.load()
    kept, _ = criteria.apply(sources.load(), crit)
    w = want.weights()
    rows = []
    for p in kept:
        s, f = want.score(p, w, prof, crit)
        rows.append((s, p, f))
    return sorted(rows, key=lambda t: -t[0])


def cmd_jobs_scan(a: argparse.Namespace) -> int:
    skipped: list[str] = []

    def warn_missing(warning: str) -> None:
        skipped.append(warning)
        say(f"warn: {warning}")

    ps = sources.scan(no_cache=a.no_cache, on_skip=warn_missing)
    kept, rejected = criteria.apply(ps)
    completed = len(sources.configured_sources()) - len(skipped)
    skipped_note = f"{len(skipped)} skipped; " if skipped else ""
    say(f"scanned {len(ps)} posting(s) from {completed} source(s); {skipped_note}{len(kept)} pass your criteria")
    if rejected:
        say("filtered out: " + ", ".join(f"{k}={v}" for k, v in rejected.most_common()))
    return 0


def cmd_jobs_interview_gaps(a: argparse.Namespace) -> int:
    from . import interview

    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    tiered = [(t, r["company"], r.get("want", 0)) for t in ("dream", "target", "practice") for r in (plan.get("tiers") or {}).get(t, [])]
    if not tiered:
        tiered = [("unplanned", s[1].company, s[0]) for s in _scored()[: a.top]]
    seen, missing = set(), []
    for tier, company, want_score in tiered:
        if company.casefold() in seen:
            continue
        seen.add(company.casefold())
        if not interview.find_company(company):
            missing.append((tier, company, want_score))
    say(f"{len(seen) - len(missing)}/{len(seen)} pipeline companies have a sourced interview loop")
    for tier, company, want_score in missing:
        say(f"  research next: {company} ({tier}, Want {want_score}) -> ask your agent: research how {company} interviews (company-research skill)")
    return 0


def cmd_jobs_preset(a: argparse.Namespace) -> int:
    from .jobs import platforms

    src_path = paths.sub("jobs", "sources.yaml")
    crit_path = paths.sub("jobs", "criteria.yaml")
    current = (store.read_yaml(src_path, {}) or {}).get("sources") or []
    if current and not a.force:
        say(f"{src_path} already lists {len(current)} source(s); rerun with --force to replace them")
        return 1
    store.write_yaml(src_path, platforms.REMOTE_PRESET)
    crit = store.read_yaml(crit_path, {}) or {}
    crit.update(platforms.REMOTE_CRITERIA)
    store.write_yaml(crit_path, crit)
    say("Remote preset written: Himalayas, Remotive, Remote OK, We Work Remotely, HN Who is hiring, Adzuna (set its country and where for roles near you)")
    say("criteria: regions global (remote roles limited to regions you did not allow are filtered out; a remote role whose location is only a country name reads as global unless that country is one of your labels in regions_ok)")
    say("Adzuna needs a free key: careerbrew secret set careerbrew.adzuna.id && careerbrew secret set careerbrew.adzuna.key")
    return 0


def cmd_jobs_list(a: argparse.Namespace) -> int:
    rated = want.ratings()
    rows = _scored()[: a.top]
    if a.json:
        say(json.dumps([{"id": p.id, "company": p.company, "title": p.title, "want": s, "features": f, "rating": rated.get(p.id)} for s, p, f in rows], indent=2))
        return 0
    for s, p, f in rows:
        top = ", ".join(f"{k} {v:.1f}" for k, v in sorted(f.items(), key=lambda kv: -kv[1])[:3])
        say(f"{s:5.1f}  {p.company:<16} {p.title[:48]:<48} [{p.id}] rated={rated.get(p.id, '-')}  ({top})")
    credits = sorted({p.source_name for _s, p, _f in rows if p.source_name})
    if credits:
        say("jobs via " + ", ".join(credits) + " (links go to each source's own page, per their terms)")
    say(f"rate with: careerbrew jobs rate <id> <1-5>   ({len(rated)} rated so far; 5+ ratings let `jobs learn` refit your Want weights)")
    return 0


def cmd_jobs_rate(a: argparse.Namespace) -> int:
    known = {p.id for p in sources.load()}
    if a.id not in known:
        say(f"unknown posting id {a.id} (see `careerbrew jobs list`)")
        return 2
    want.rate(a.id, a.rating, a.tags.split(",") if a.tags else [])
    say(f"rated {a.id} = {a.rating}")
    n = len(want.ratings())
    if n >= want.AUTO_LEARN_EVERY and n % want.AUTO_LEARN_EVERY == 0:  # issue #185: ratings refit Want on their own
        res = want.learn_weights(sources.load(), want.profile(), criteria.load())
        if res["status"] == "kept":
            say(f"{n} ratings: your Want weights were refit (agreement {res['before']} -> {res['after']}); `jobs list` uses them now")
        elif res["status"] == "reverted":
            say(f"{n} ratings: a refit agreed less with you ({res['before']} -> {res['after']}), so the weights stay")
    return 0


def cmd_jobs_learn(_a: argparse.Namespace) -> int:
    res = want.learn_weights(sources.load(), want.profile(), criteria.load())
    say(json.dumps(res))
    return 0


def cmd_jobs_choose(a: argparse.Namespace) -> int:
    """You choose the jobs; careerbrew only suggests."""
    machine.guard("jobs")
    try:
        picks = strategy.choose(a.posting, a.tier)
    except (KeyError, ValueError) as e:
        say(str(e).strip("'\""))
        return 1
    say(f"chosen: {a.posting} as {a.tier} ({len(picks)} job(s) chosen). Run `careerbrew jobs strategy` to re-plan.")
    return 0


def cmd_jobs_unchoose(a: argparse.Namespace) -> int:
    machine.guard("jobs")
    picks = strategy.unchoose(a.posting)
    say(f"removed {a.posting}; {len(picks)} job(s) chosen")
    return 0


def cmd_jobs_strategy(a: argparse.Namespace) -> int:
    plan = strategy.build()
    if a.json:
        say(json.dumps(plan, indent=2, default=str))
        return 0
    say(f"phase: {plan['phase']}  readiness: {plan['readiness']:.0%}")
    for s in plan["steps"]:
        say(f"- {s}")
    say("tiers: " + ", ".join(f"{t}={len(v)}" for t, v in plan["tiers"].items()))
    say(f"gaps: {', '.join(s for s, _ in plan['skill_gaps']) or 'none'} (added as drill items)")
    say(f"full plan: {paths.sub('out', 'strategy.md')}")
    return 0


def cmd_jobs_log(a: argparse.Namespace) -> int:
    strategy.log_application(a.company, a.tier, a.stage)
    say(f"logged {a.tier} {a.stage}")
    return 0


# ---------- drill ----------
def _print_item(it: dict[str, Any]) -> None:
    rubric = tutor.rubrics()[it["rubric"]]
    say(f"[{it['mode']}] {it['id']} · topic: {it['topic']} · round: {it['round']}")
    if it["mode"] == "learn" and it.get("teach"):
        say(f"Lesson: {it['teach']}")
    say(f"Question: {it['prompt']}")
    say("Rubric (binary): " + "; ".join(f"{c['id']}: {c['text']}" for c in rubric["criteria"]))


def cmd_drill_next(a: argparse.Namespace) -> int:
    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    focus = strategy.CORE_TOPICS + [s for s, _ in plan.get("skill_gaps", [])]
    it = tutor.next_item(focus)
    if it is None:
        say("nothing due and no new items. Come back when reviews are due.")
        return 0
    if a.json:
        say(json.dumps(it, indent=2))
    else:
        _print_item(it)
        say(f"grade with: careerbrew drill answer {it['id']} --met <criterion,ids>")
    return 0


def cmd_drill_answer(a: argparse.Namespace) -> int:
    met = [m for m in (a.met or "").split(",") if m]
    row = tutor.record(a.item, met, seconds=a.seconds)
    for line in tutor.feedback(row):
        say(line)
    return 0


def cmd_drill_speak(a: argparse.Namespace) -> int:
    """Voice practice: ask the question (aloud with --aloud), transcribe the spoken answer locally, grade it."""
    import shutil
    import subprocess

    from .tutor import voice

    item = tutor.items().get(a.item)
    if item is None:
        say(f"unknown item {a.item} (see `careerbrew drill list`)")
        return 1
    say(f"Question: {item['prompt']}")
    if a.aloud and shutil.which("say"):
        subprocess.run(["say", item["prompt"]], check=False, timeout=120)  # macOS speech, on this machine
    try:
        out = voice.answer(a.item, text=a.text, audio=Path(a.audio).expanduser() if a.audio else None, seconds=a.seconds)
    except voice.VoiceError as e:
        say(f"error: {e}")
        return 1
    if out["source"] == "audio":
        say(f"Transcript: {out['transcript']}")
    say("Met: " + (", ".join(out["row"]["met"]) or "none"))
    for line in out["feedback"]:
        say(line)
    return 0


def cmd_drill_session(a: argparse.Namespace) -> int:
    """Interactive loop for a human at the terminal: question, answer, self-grade per criterion."""
    done = 0
    while done < a.count:
        it = tutor.next_item(strategy.CORE_TOPICS)
        if it is None:
            break
        _print_item(it)
        met = []
        try:
            input("Answer out loud or on paper, then press Enter to grade... ")
            for c in tutor.rubrics()[it["rubric"]]["criteria"]:
                if input(f"  {c['text']}? [y/N] ").strip().lower().startswith("y"):
                    met.append(c["id"])
        except EOFError:  # input ended mid-question: stop, record nothing half-graded
            say("")
            break
        except KeyboardInterrupt:  # Ctrl-C: the same stop, with the usual exit code
            say("")
            say(f"session stopped: {done} item(s)")
            return 130
        for line in tutor.feedback(tutor.record(it["id"], met, graded_by="self")):
            say(line)
        done += 1
    say(f"session done: {done} item(s). Progress: careerbrew drill progress")
    return 0


# ---------- flashcards / library / import ----------
def _card_line(c: dict[str, Any]) -> str:
    return f"[{c['mode']}] {c['topic']}  ({c['id']})"


def cmd_cards_review(a: argparse.Namespace) -> int:
    """Recall before reveal: question, your answer from memory, then the answer and your grade."""
    from .tutor import cards

    done = 0
    keys = {"1": "again", "2": "hard", "3": "good", "4": "easy"}
    while done < a.count:
        q = cards.queue(limit=1)
        if not q:
            break
        c = q[0]
        say(_card_line(c))
        say(f"Q: {c['question']}")
        try:
            if input("Answer from memory (out loud or on paper), then press Enter to see the answer (q quits)... ").strip().lower() == "q":
                break
            say(f"A: {c['answer']}")
            grade = ""
            while grade not in keys:
                grade = input("How well did you recall it? 1 again, 2 hard, 3 good, 4 easy (q quits): ").strip().lower()
                if grade == "q":
                    break
        except (EOFError, KeyboardInterrupt):
            say("")
            break
        if grade == "q":
            break
        row = cards.review(c["id"], keys[grade])
        say(f"next review {row['due'][:10]}" + ("; leech: suspended until you rewrite it (careerbrew cards unsuspend)" if row["leech"] else ""))
        done += 1
    s = cards.stats()
    say(f"session done: {done} card(s); due now {s['due_now']}, reviewed today {s['reviewed_today']}")
    return 0


def cmd_cards_next(a: argparse.Namespace) -> int:
    from .tutor import cards

    q = cards.queue(limit=a.count)
    if a.json:
        say(json.dumps([{k: c[k] for k in ("id", "topic", "mode", "question")} for c in q], indent=2))
    else:
        for c in q:
            say(f"{_card_line(c)}  {c['question']}")
        say(f"{len(q)} card(s) queued. Review: careerbrew cards review")
    return 0


def cmd_cards_grade(a: argparse.Namespace) -> int:
    from .tutor import cards

    row = cards.review(a.id, a.rating)
    say(f"{a.id}: {a.rating}; next review {row['due'][:10]}" + (" (leech: suspended)" if row["leech"] else ""))
    return 0


def cmd_cards_add(a: argparse.Namespace) -> int:
    from .tutor import cards

    c = cards.add(a.question, a.answer, [t for t in a.tags.split(",") if t], deck=a.deck)
    say(f"added card {c['id']} to deck {c['deck']}")
    return 0


def cmd_cards_stats(_a: argparse.Namespace) -> int:
    from .tutor import cards

    s = cards.stats()
    ret = "n/a" if s["retention_30d"] is None else f"{s['retention_30d']:.0%}"
    say(f"cards {s['total']}  new {s['new']}  due now {s['due_now']}  due today {s['due_today']}  reviewed today {s['reviewed_today']}  retention (30 d) {ret}  streak {s['streak_days']} d  leeches {len(s['suspended'])}")
    for t, r in sorted(s["by_topic"].items(), key=lambda kv: -kv[1]["total"])[:12]:
        say(f"  {t:<24} {r['total']:>4} cards  {r['due']:>3} due  {r['new']:>4} new")
    return 0


def cmd_cards_explain_queue(a: argparse.Namespace) -> int:
    """What the card-explainer skill should write next. Prints questions (and technical hints) for the agent; behavioural answers never leave."""
    from .tutor import public

    machine.guard("jobs")
    items = public.explain_queue()
    if a.topic:
        items = [i for i in items if i["topic"] == a.topic.casefold()]
    items = items[a.offset : a.offset + a.count]
    if a.json:
        print(json.dumps(items, indent=1, ensure_ascii=False))
    else:
        by: dict[str, int] = {}
        for i in public.explain_queue():
            by[i["reason"]] = by.get(i["reason"], 0) + 1
        say(f"explain queue: {sum(by.values())} card(s) " + ", ".join(f"{k}: {v}" for k, v in sorted(by.items())) + f" · prompt {public.prompt_version()}")
    return 0


def cmd_cards_publish(a: argparse.Namespace) -> int:
    """Validate generated cards and merge them into knowledge/cards/. The input file is deleted after a successful publish."""
    import yaml

    from .tutor import public

    src = Path(a.file)
    rows = yaml.safe_load(store.read_text(src) or "") or []
    if isinstance(rows, dict):
        rows = rows.get("cards", [])
    try:
        r = public.publish(rows, dry_run=a.dry_run)
    except public.PublishError as e:
        say(str(e))
        return 1
    if a.dry_run:
        say(f"dry run: {r['published']} public card(s) and {r['private']} private explanation(s) pass the bar, {r['skipped']} marked personal; nothing written")
        return 0
    if not a.keep:
        src.unlink(missing_ok=True)
    events.emit("cards.publish", status="ok", items=r["published"])
    say(f"published {r['published']} card(s), skipped {r['skipped']} as personal, {r['private']} private explanation(s) saved; public deck: {r['total']} card(s) in knowledge/cards/")
    return 0


def cmd_cards_public_check(_a: argparse.Namespace) -> int:
    from .tutor import public

    problems = public.check()
    for p in problems:
        say(p)
    say(f"public cards: {len(public.load())} card(s), {len(problems)} problem(s)")
    return 1 if problems else 0


def cmd_exercises_check(a: argparse.Namespace) -> int:
    """Gate: every exercise's reference solution passes its own tests and complexity check."""
    from .tutor import exercises

    if a.file:
        import yaml

        ex = yaml.safe_load(Path(a.file).read_text(encoding="utf-8"))
        rows = ex if isinstance(ex, list) else [ex]
        problems = [f"{e.get('id')}: {p}" for e in rows for p in exercises.validate(e)]
        n = len(rows)
    else:
        problems = exercises.check_all()
        n = len(exercises.load())
    for p in problems:
        say(p)
    say(f"exercises: {n} checked, {len(problems)} problem(s)")
    return 1 if problems else 0


def cmd_exercises_list(_a: argparse.Namespace) -> int:
    from .tutor import exercises

    st = exercises.status()
    for e in exercises.load():
        s = st.get(e["id"], {})
        mark = "solved" if s.get("solved") else "synced" if s.get("synced") else f"{s.get('attempts', 0)} tries" if s.get("attempts") else ""
        say(f"{e['order']:>3}  {e['pattern']:<24} {e['difficulty']:<6} {e['id']:<36} {mark}")
    return 0


def cmd_exercises_submit(a: argparse.Namespace) -> int:
    from .tutor import exercises

    machine.guard("jobs")
    r = exercises.submit(a.id, Path(a.file).read_text(encoding="utf-8"), seconds=a.seconds)
    g = r["grade"]
    say(f"{g['passed']}/{g['total']} tests · score {g['score']}" + (f" · complexity {g['complexity']['expected']}: {'ok' if g['complexity']['ok'] else 'too slow'}" if g["complexity"] else ""))
    if r["error"]:
        say(f"error: {r['error']}")
    for m in r["mistakes"]:
        say(f"{'repeat ' if m['repeat'] else ''}mistake ({m['kind']}): {m['tip']}")
    return 0 if g["correct"] else 1


def _owner_terminal() -> bool:
    """Goals belong to the owner: the CLI changes them only on an interactive terminal (agents run without one)."""
    if sys.stdin.isatty() or _owner_ui() or os.environ.get("CAREERBREW_TEST_OWNER") == "1":
        return True
    say("only you set goals: run this in your own terminal, or use Goals in the web app")
    return False


def cmd_goals_show(_a: argparse.Namespace) -> int:
    from . import goals

    g = goals.load()
    for lt in g["long_term"]:
        say(f"long-term: {lt['text']}" + (f" (by {lt['by']})" if lt.get("by") else ""))
    for q in g["queue"]:
        mark = "→" if g["upcoming"] and q["id"] == g["upcoming"]["id"] else " "
        say(f"{mark} {q['title']} [{q['kind']}]" + (f" · {q['date']}" if q.get("date") else ""))
    up = g["upcoming"]
    if up:
        left = goals.days_left(up)
        say(f"upcoming: {up['title']} [{up['kind']}]" + (f" · {left} day(s) left" if left is not None else "") + f" · {up['minutes_per_day']} min/day")
        say(f"focus: {goals.focus(up)['why']}")
    else:
        say("no upcoming goal: `careerbrew goals set` (or Goals in the web app)")
    return 0


def cmd_goals_set(a: argparse.Namespace) -> int:
    from . import goals

    if not _owner_terminal():
        return 1
    try:
        r = goals.set_upcoming({"kind": a.kind, "title": a.title, "date": a.date, "company": a.company, "topics": [t.strip() for t in a.topics.split(",") if t.strip()], "minutes_per_day": a.minutes})
    except goals.GoalError as e:
        say(str(e))
        return 1
    blocks = [s for s in r["plan"]["schedule"] if s.get("goal")]
    say(f"upcoming goal set: {r['upcoming']['title']}. Today re-planned: {len(blocks)} study block(s) first.")
    return 0


def cmd_goals_long_term(a: argparse.Namespace) -> int:
    from . import goals

    if not _owner_terminal():
        return 1
    items = goals.load()["long_term"] + [{"text": a.text, "by": a.by}]
    goals.set_long_term(items)
    say(f"long-term goals: {len(items)}")
    return 0


def cmd_approvals_request(a: argparse.Namespace) -> int:
    from . import approvals

    item = approvals.request(a.title, a.detail, a.by)
    say(f"approval {item['id']} is waiting for the owner (teacher mode or `careerbrew approvals decide`). Check it with `careerbrew approvals show {item['id']}` and act only if approved.")
    if rules := approvals.guidance(5):  # issue #185: past notes are standing guidance
        say("the owner's notes on past requests (follow them):")
        for g in rules:
            say(f"  - {g['status']}: {g['title']} · {g['note']}")
    return 0


def cmd_approvals_guidance(_a: argparse.Namespace) -> int:
    from . import approvals

    rows = approvals.guidance()
    if not rows:
        say("no notes yet: a note on an approval decision becomes guidance agents read before they ask again")
        return 0
    say(f"{len(rows)} note(s) from your decisions, newest first; agents see them when they ask:")
    for g in rows:
        say(f"- {g['status']}: {g['title']} · {g['note']}")
    return 0


def cmd_approvals_list(_a: argparse.Namespace) -> int:
    from . import approvals

    for item in approvals.load()[-20:]:
        say(f"{item['id']}  {item['status']:<9} {item['title']}")
    return 0


def cmd_approvals_show(a: argparse.Namespace) -> int:
    from . import approvals

    item = approvals.get(a.id)
    say(f"{item['id']} {item['status']}: {item['title']}" + (f" · note: {item['note']}" if item.get("note") else ""))
    return 0 if item["status"] == "approved" else 1


def cmd_approvals_decide(a: argparse.Namespace) -> int:
    from . import approvals

    if not _owner_terminal():
        return 1
    item = approvals.decide(a.id, a.decision, a.note)
    say(f"{item['id']} {item['status']}")
    return 0


def cmd_teach_next(a: argparse.Namespace) -> int:
    from . import teacher

    step = teacher.next_step()
    if a.json:
        print(json.dumps(step, indent=1, ensure_ascii=False, default=str))
    else:
        say(f"next: {step['title']}\nwhy: {step['why']}")
    return 0


def cmd_teach_improvements(_a: argparse.Namespace) -> int:
    from . import teacher

    items = teacher.improvements()
    for i in items:
        say(f"[{i['signal']}] {i['proposal']}")
    say(f"{len(items)} improvement(s) proposed · teaching rules {teacher.rules_version()}")
    return 0


def cmd_teach_feedback(a: argparse.Namespace) -> int:
    from . import teacher

    teacher.feedback(a.text, a.step or "")
    say("thanks: it becomes a proposal in `careerbrew teach improvements`")
    return 0


def cmd_exercises_sync(a: argparse.Namespace) -> int:
    from .tutor import exercises

    machine.guard("jobs")
    try:
        r = exercises.sync_leetcode(a.username) if a.username else exercises.mark_solved(a.solved, source="manual")
    except ValueError as e:
        say(str(e))
        return 1
    except OSError as e:  # URLError, timeouts, proxy refusals
        say(f"LeetCode is unreachable from this machine ({type(e).__name__}); nothing changed. Check the network and try again.")
        return 1
    if a.username and not r.get("seen"):
        total = exercises.leetcode_profile_solved(a.username)
        say(
            f"LeetCode returned no recent accepted submissions{f' (your profile shows {total} solved)' if total else ''}. LeetCode only lists recent solves, and only when the profile shows them; mark older ones with `careerbrew exercises sync <slug> ...`."
        )
        return 0
    say(f"marked solved: {r['matched']} exercise(s) ({r['new']} new; {r['total']} total){' · not in the curriculum: ' + str(r['unknown']) if r.get('unknown') else ''}")
    return 0


def cmd_cards_unsuspend(a: argparse.Namespace) -> int:
    from .tutor import cards

    cards.unsuspend(a.id)
    say(f"{a.id} is back in the queue")
    return 0


def cmd_import_notion(a: argparse.Namespace) -> int:
    from . import notion_import

    folder = Path(a.dir) if a.dir else None
    if a.check:
        chk = notion_import.check(folder)
        say(f"notion import: {chk['imported']}/{chk['expected']} items present ({', '.join(f'{k} {v}' for k, v in chk['by_kind'].items())}); blank cards skipped: {chk['skipped_empty']}")
        return 0 if chk["ok"] else 1
    r = notion_import.run(folder)
    say("imported from notion: " + ", ".join(f"{k} {v}" for k, v in r["counts"].items()))
    if r["unmapped_stages"]:
        say(f"warning: job stages with no application-log mapping (board only): {', '.join(r['unmapped_stages'])}")
    say(f"cards in the notion deck: {r['cards']} (blank cards skipped: {r['skipped_empty']}); applications logged: {r['applications']}; skills added: {r['skills_added']}; leak terms added: {r['leak_terms_added']}")
    return 0


def cmd_library_list(_a: argparse.Namespace) -> int:
    from . import notion_import

    for e in notion_import.library():
        say(f"{e['section']:<9} {e['id'][:8]}  {e['title']}")
    return 0


def cmd_library_show(a: argparse.Namespace) -> int:
    from . import notion_import

    p = notion_import.page(a.id) or next((notion_import.page(e["id"]) for e in notion_import.library() if e["id"].startswith(a.id)), None)
    if p is None:
        raise KeyError(f"no library page {a.id} (see careerbrew library list)")
    say(f"# {p['title']}\n\n{p['markdown']}")
    return 0


def cmd_drill_progress(_a: argparse.Namespace) -> int:
    rep = Report("Interview readiness", "Spaced repetition (FSRS) with binary rubrics. Private; stays on this machine.")
    m = tutor.mastery()
    atts = tutor.attempts()
    due = sum(1 for c in tutor.cards().values() if c.due <= tutor.now())
    rep.kpis([("readiness (core)", f"{tutor.readiness(strategy.CORE_TOPICS):.0%}"), ("attempts", str(len(atts))), ("due now", str(due))])
    rep.bars("Mastery by topic", sorted(m.items(), key=lambda kv: -kv[1]))
    rep.line("Rubric score per attempt", [(x["at"][:16].replace("T", " "), x["score"]) for x in atts])
    hist = store.read_jsonl(paths.sub("tutor", "readiness.jsonl"))
    rep.line("Readiness over time", [(h["at"][:16].replace("T", " "), h["readiness"]) for h in hist])
    missed: dict[str, int] = {}
    for x in atts:
        for c in x["missed"]:
            missed[c] = missed.get(c, 0) + 1
    rep.bars("Most-missed rubric criteria (count)", sorted(missed.items(), key=lambda kv: -kv[1])[:8], max_value=max(missed.values(), default=1), fmt="{:.0f}")
    out = paths.sub("out", "progress.html")
    out.write_text(rep.html(), encoding="utf-8")
    say(f"readiness {tutor.readiness(strategy.CORE_TOPICS):.0%} over {len(atts)} attempt(s); chart: {out}")
    return 0


def cmd_drill_list(_a: argparse.Namespace) -> int:
    for iid, it in sorted(tutor.items().items()):
        say(f"{iid:<28} {it['topic']:<18} {it['round']:<14} {it['origin']}")
    return 0


# ---------- work ----------
def _memory_notes(out: Callable[[str], None] | None = None) -> dict[str, Any]:
    """After a collect: work items that match a project go into its memory file as dated notes (business reading rule applies).

    `out` is where the count lines go (default stdout); a long run passes its `run_say()`, so they land on its page."""
    from .work import memory_sync

    out = out or say
    r = memory_sync.sync_out(allowed=may_show_business())
    if r["notes"]:
        out(f"project memory: {r['notes']} note(s) written to {r['projects']} project(s)")
    if r["removed"]:
        out(f"project memory: {r['removed']} note(s) of events now personal removed")
    return r


def cmd_work_collect(_a: argparse.Namespace) -> int:
    from . import report, runs

    step = run_say()
    res = evidence.collect()
    step(f"evidence: {res['new']} new, {res['total']} total, {res['errors']} source error(s)")
    for reason in res.get("reasons", []):
        step(f"  source error {reason}")
    memory = _memory_notes(step)
    report.finish(runs.work_collect(res, memory), say)
    return 1 if res["errors"] else 0


def cmd_work_note(a: argparse.Namespace) -> int:
    it = evidence.note(a.title, a.tags.split(",") if a.tags else [], a.impact or "")
    say(f"noted {it['id']} tags={','.join(it['tags'])}")
    return 0


def _assessment(a: argparse.Namespace) -> dict[str, Any]:
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    ladder = a.ladder or settings.get("ladder", "generic-swe")
    level = a.level or settings.get("target_level", "senior")
    return promo.assess(ladder, level)


def cmd_work_promo(a: argparse.Namespace) -> int:
    asm = _assessment(a)
    path = promo.write_doc(asm)
    rep = Report(f"Promotion case: {asm['level']}", f"Ladder {asm['ladder']} · coverage of the bar per competency")
    rep.kpis([("overall coverage", f"{asm['overall']:.0%}"), ("gaps", str(len(promo.gaps(asm))))])
    rep.bars("Coverage by competency", [(r["name"], r["coverage"]) for r in asm["competencies"]])
    html_path = paths.sub("out", f"promo-{asm['ladder']}-{asm['level']}.html")
    html_path.write_text(rep.html(), encoding="utf-8")
    say(f"overall coverage {asm['overall']:.0%} for {asm['level']} on {asm['ladder']}")
    for w in asm["warnings"]:
        say(f"warning: {w}")
    say(f"doc: {path}\nchart: {html_path}")
    if a.show:
        if not may_show_business():
            say("refusing --show: stdout is not a terminal and no plugin policy allows agents to read business data")
            return 3
        say(path.read_text(encoding="utf-8"))
    return 0


def _say_stale_sources(limit: int = 3) -> None:
    """Stale project sources come first: work from old notes is the costly mistake (issue #32)."""
    from .work import projects

    stale = projects.stale_sources()
    for s in stale[:limit]:
        say(f"stale: {s['project']} {s['kind']} {s['ref']} ({_age(s['age_days'])}, limit {s['limit']}d) -> read it, then careerbrew work project check {s['project']} {s['ref']}")
    if len(stale) > limit:
        say(f"stale: {len(stale) - limit} more (careerbrew work project list)")


def cmd_work_focus(_a: argparse.Namespace) -> int:
    from .work import focus

    _say_stale_sources()
    d = focus.build()
    show = may_show_business()
    for h in d["horizons"]:
        if not h["items"]:
            say(f"{h['label']}: nothing yet. {h['empty']}")
            continue
        top = h["items"][0]
        say(f"{h['label']}: {top['title'] if show else (top.get('ref') or 'see the web app')}")
        say(f"  why: {top['why'] if show else 'run in your terminal or open the Dashboard view'}")
    events.emit("work.focus", status="ok", horizons=sum(1 for h in d["horizons"] if h["items"]))
    return 0


def _age(days: float | None) -> str:
    return "never checked" if days is None else f"{days:g}d ago"


def cmd_work_priority(a: argparse.Namespace) -> int:
    """The ranked project list that the PM update, the plan, and Next step follow (issue #68)."""
    from .work import project_priority

    if a.action in ("set", "remove"):
        project_priority.set_project(a.project, a.rank, a.match) if a.action == "set" else project_priority.remove(a.project)
        say("saved: the PM update and the web app use it now; rebuild today's saved plan for `day next` with: careerbrew day plan")
    rows = project_priority.summary()
    if not rows:
        say("no project priority yet: careerbrew work priority set <project> --match '<glob>' (globs match repos like */agentbrew, ticket keys like PROJ-*, or titles like *invoice*)")
        return 0
    say("project priority, rank 1 first (the PM update, the plan, and Next step follow it):")
    for r in rows:
        say(f"  {r['rank']}. {r['name']:<16} {', '.join(r['match']):<32} {r['open_tasks']} open task{'' if r['open_tasks'] == 1 else 's'}, {r['today']} item{'' if r['today'] == 1 else 's'} today")
    say("change it: careerbrew work priority set <project> --rank <n> [--match <glob> ...], or careerbrew work priority remove <project>")
    return 0


def cmd_work_agent(a: argparse.Namespace) -> int:
    """Agent activity (issue #272): a background agent reports each step; Work's Agent activity shows it live."""
    from .work import agents

    if a.action == "event":
        try:
            run, project = a.run, a.project
            if getattr(a, "session", ""):  # a project session's step (issue #274): its run is the session, its project the session's
                from .work import sessions

                try:
                    s = sessions.live(a.session)
                except sessions.SessionError as e:
                    say(f"error: {e} (breaks {sessions.STORY})")
                    return 2
                if (run and run != s["id"]) or (project and project != s["project"]):
                    raise agents.AgentError("--session sets the run and the project: leave out --run and --project")
                run, project = s["id"], s["project"]
            row = agents.record(a.agent, a.title, a.status, a.url, project, run)
        except agents.AgentError as e:
            say(f"error: {e} (breaks {agents.STORY})")
            return 2
        say(f"event stored: {row['agent']} {row['status']}" + (f" (run {row['run']})" if row["run"] != row["agent"] else "") + "; Agent activity on Work shows it within seconds")
        return 0
    rows, show = agents.runs(), may_show_business()
    if a.json:
        keep = ("agent", "run", "project", "status", "at", "started_at", "events") + (("title", "url") if show else ())
        say(json.dumps({"counts": agents.counts(rows), "runs": [{k: r[k] for k in keep} for r in rows]}))
        return 0
    if not rows:
        say('no agent activity yet: an agent reports each step with careerbrew work agent event <agent> "<what it did>" [--project <name>] [--url <link>]; --status done ends the run')
        return 0
    c = agents.counts(rows)
    say(f"{len(rows)} run(s): {c['running']} running, {c['quiet']} quiet, {c['done']} done, {c['failed']} failed")
    for r in rows:
        run = f" ({r['run']})" if r["run"] != r["agent"] else ""
        say(f"  {r['status']:<8} {r['agent']}{run}  {r['project'] or '(no project)'}  {r['events']} event(s), last {r['at']}" + (f"  {r['title']}" + (f"  {r['url']}" if r["url"] else "") if show else ""))
    if not show:
        say("titles and links hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
    return 0


def _session_line(r: dict[str, Any], show: bool) -> str:
    act = r["activity"]
    last = f"last event {act['status']} at {act['at'][11:16]}" if act["events"] else "no event yet"
    if r["has_handoff"]:
        handoff_status = "written"
    elif r["handoff"]:
        handoff_status = "stale or unverified: write a fresh handoff"
    else:
        handoff_status = "not yet"
    head = f"  {r['state']:<6} {r['id']}" + (f"  {r['project']}" if show else "") + f"  {last}, {len(r['claims'])} claim(s), {r['questions']} open question(s), handoff {handoff_status}"
    if show and r["handoff"]:
        head += f"  handoff: {r['handoff']}"
    return head


def cmd_work_session(a: argparse.Namespace) -> int:
    """Project sessions (issue #274): one long-lived session per project, talking through the store."""
    from . import config
    from .work import sessions
    from .workday import questions

    show = may_show_business()
    try:
        if a.action == "open":
            s = sessions.open_session(a.project, a.id or "")
            if a.json:
                result = {k: s[k] for k in ("id", "day", "started", "reused")}
                if s.get("brief_status"):
                    result["brief_status"] = s["brief_status"]
                if show and s.get("brief"):
                    result["brief"] = s["brief"]
                print(json.dumps(result))
                return 0
            how = "reused: the project's live session today" if s["reused"] else "opened and recorded as a session loop"
            say(f"session {s['id']} {how}; it writes through careerbrew (careerbrew day run --json, `sessions`)")
            if s.get("brief_status") == "legacy-unverified":
                say("prior session history is unverified; check the session archive, subject to agent_keep_days")
            if show and s.get("brief"):
                say(f"project handoff: {s['brief']}")
            return 0
        if a.action == "ask":
            q = sessions.ask(a.id, a.text, a.option, a.pick or "", a.recheck or "")
            say(f"question {q['id']} saved for {q['step']} ({questions.kind(q['recheck'])} recheck); the main session asks it in the one batched ask (careerbrew day question recheck)")
            return 0
        if a.action == "handoff":
            s = sessions.write_handoff(a.id, a.text, a.link or "")
            say(f"handoff written for session {s['id']} (Project sessions in careerbrew day handoff show); end it: careerbrew work session end {s['id']}")
            return 0
        if a.action == "end":
            if bool(a.id) == bool(a.all):
                raise sessions.SessionError("end needs a session id (or its project) or --all")
            got = sessions.end(a.id or "", every=a.all)
            if a.json:
                print(json.dumps({"ended": [s["id"] for s in got["ended"]], "missing": [s["id"] for s in got["missing"]], "unverified_loops": [s["id"] for s in got["ended"] if not s.get("record_id")]}))
                return 1 if got["missing"] else 0
            for s in got["ended"]:
                say(f"  ended {s['id']}: archived with its handoff, claims released")
                if not s.get("record_id"):
                    say(f"  older loop ownership is unverified: check the host loop, then run careerbrew day loop stop --id {s['id']} if it is stopped")
            for s in got["missing"]:
                remedy = "write a fresh handoff" if s.get("handoff") else "write its handoff first"
                say(f'  still open {s["id"]}: {remedy}: careerbrew work session handoff {s["id"]} "<where it stands; the next step>"')
            say(f"sessions: {len(got['ended'])} ended, {len(got['missing'])} still open without a current handoff")
            return 1 if got["missing"] else 0
    except sessions.SessionError as e:
        say(f"error: {e} (breaks {sessions.STORY})")
        return 1 if isinstance(e, sessions.ClaimHeld) else 2
    rows = sessions.listing(include_ended=a.all)
    if a.json:
        print(json.dumps({"cap": int(config.get("work_session_cap")), "sessions": rows if show else [sessions.public(r) for r in rows]}))
        return 0
    if not rows:
        say("no project sessions: the /work agent opens one per pressing project with careerbrew work session open <project> --id <session id>")
        return 0
    for r in rows:
        say(_session_line(r, show))
    n = sum(1 for r in rows if r["state"] == "live")
    say(f"sessions: {n} of {config.get('work_session_cap')} live" + (f", {sum(1 for r in rows if r['state'] == 'stale')} left from an earlier day" if any(r["state"] == "stale" for r in rows) else ""))
    if not show:
        say("projects and handoffs hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
    return 0


def cmd_work_claim(a: argparse.Namespace) -> int:
    """Claims (issue #274): a session's recorded, expiring hold on a shared thread or draft; other sessions skip a claimed one."""
    from .work import sessions

    try:
        if a.action == "take":
            c = sessions.take(a.resource, a.session, a.minutes)
            say(f"{'renewed' if c['renewed'] else 'claimed'} {c['resource']} for session {c['session']} until {c['until'][11:16]}; release it when done: careerbrew work claim release {c['resource']} --session {c['session']}")
            return 0
        if a.action == "release":
            if not sessions.release(a.resource, a.session):
                raise sessions.SessionError(f"session {a.session} holds no claim on {a.resource} (careerbrew work claim list)")
            say(f"released {a.resource}")
            return 0
    except sessions.SessionError as e:
        say(f"error: {e} (breaks {sessions.STORY})")
        return 1 if isinstance(e, sessions.ClaimHeld) else 2
    rows = sessions.claims()
    show = may_show_business()
    if a.json:
        print(json.dumps({"claims": rows if show else [{k: c[k] for k in ("session", "at", "until")} | {"kind": c["resource"].split(":", 1)[0]} for c in rows]}))
        return 0
    for c in rows:
        say(f"  {c['resource'] if show else c['resource'].split(':', 1)[0] + ':(hidden)'}  session {c['session']}  until {c['until'][11:16]}")
    say(f"claims: {len(rows)} held")
    return 0


def cmd_work_feed(a: argparse.Namespace) -> int:
    """The work feed (issue #72): one local store for Slack asks, PagerDuty pages, Outlook mail and meetings, and plugin events."""
    from .work import feed

    if a.action == "pull":
        r = feed.pull()
        if not r["sources"]:
            say("no feed connectors: careerbrew connect slack | pagerduty | outlook, then careerbrew work feed pull")
            return 0
        say(f"pulled {r['sources']} source(s): {r['new']} new, {r['updated']} updated, {r['errors']} error(s)")
        for reason in r["reasons"]:
            say(f"  {reason}")
        _memory_notes()
        return 1 if r["errors"] else 0
    if a.action == "add":
        text = sys.stdin.read() if a.file == "-" else Path(a.file).expanduser().read_text(encoding="utf-8")
        r = feed.add_lines(text, a.plugin or "", a.source or "")
        skipped = f", {r['skipped']} skipped (no id, time, or title)" if r["skipped"] else ""
        say(f"added {r['new']} event(s), updated {r['updated']}{skipped}; originals kept encrypted, apart from the analysis (see: careerbrew work feed show)")
        _memory_notes()
        return 0
    if a.action == "transcript":
        from .work import transcript

        r = transcript.store(Path(a.file), a.since or "")
        say(f"stored {r['new']} new event(s), {r['updated']} updated, {r['already']} already stored, {r['skipped']} skipped (no id, time, or title)")
        say(
            "replies: "
            + ", ".join(f"{s}={n}" for s, n in r["replies"].items())
            + f"; {r['errors']} error(s); saved-output files: {r['pointers']['followed']} read, {r['pointers']['refused']} refused (outside the transcript's folder or missing)"
        )
        for s in r["gaps"]:
            say(f"gap  {s}: nothing stored this cycle (fix: run its tool again, or check its replies have id, at, and title)")
        if r["new"] or r["updated"]:
            _memory_notes()
        return 1 if r["gaps"] else 0
    if a.action == "cycle":
        r = feed.mark_cycle([x.strip() for x in a.require.split(",") if x.strip()] if a.require else None)
        if a.json:
            say(json.dumps(r))
        else:
            say(f"cycle: stored {len(set(r['required']) & set(r['stored']))} of {len(r['required'])} required source(s); coverage {len(r['required']) - len(r['not_stored'])} of {len(r['required'])}")
            for s in r["not_stored"]:
                say(f"not stored  {s}: required acquisition coverage is missing or invalid (fix: pull it again, then careerbrew work feed add or transcript)")
        return 1 if r["not_stored"] else 0
    if a.action == "mark" and a.value in (*feed.ASK_MARKS, "ask"):
        row = feed.mark_ask(a.id, a.value)
        say(
            f"{a.id}: "
            + {"not-ask": "not an ask: it leaves your open asks", "done": "done elsewhere: it leaves your open asks", "ask": "an open ask again"}[a.value]
            + ("" if a.value == "ask" else f" (undo: careerbrew work feed mark {a.id} ask)")
        )
        return 0
    if a.action == "mark":
        row = feed.mark(a.id, personal=a.value == "personal")
        say(
            f"{a.id}: {'personal: kept out of evidence, the day log, project memory, and the PM update' if row['personal'] else 'work: counts as work again'}"
            + (f"; {row['dropped']} evidence row(s) removed" if row["dropped"] else "")
            + (f"; {row['memory_dropped']} project memory note(s) removed" if row.get("memory_dropped") else "")
        )
        return 0
    if a.action == "links":
        rows = [r for r in feed.load() if feed.needs_research(r)] if a.pending else [r for r in feed.load() if r.get("links") and r["triage"] != "noise" and not r.get("personal")]
        if not rows:
            say("no kept event with links to research" if a.pending else "no kept event has links")
            return 0
        show = may_show_business()
        say(f"{len(rows)} kept event(s) with links; {sum(len(feed.needs_research(r)) for r in rows)} link(s) not researched yet")
        for r in rows[-a.top :]:
            todo = feed.needs_research(r)
            say(f"- {r['id']}  {len(r['links'])} link(s), {len(todo)} to research" + (f"  team: {', '.join(r['team'])}" if show and r.get("team") else ""))
            for u in todo if show else []:
                say(f"    {u}")
        if not show:
            say("links hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
        say("research one event: careerbrew work feed research <id> <file.json> (a list of {url, summary} or {url, opened: false, reason}, one per link)")
        return 0
    if a.action == "research":
        text = sys.stdin.read() if a.file == "-" else Path(a.file).expanduser().read_text(encoding="utf-8")
        try:
            entries = json.loads(text)
        except json.JSONDecodeError:
            say("not JSON: research is a list of {url, summary} or {url, opened: false, reason}, one per link")
            return 2
        row = feed.research(a.id, entries)
        opened = sum(e["opened"] for e in row["research"])
        say(f"{a.id}: {len(row['research'])} link(s) researched ({opened} opened, {len(row['research']) - opened} not opened, with the reason)")
        return 0
    if a.action == "retriage":
        r = feed.retriage()
        say(f"triaged {r['events']} event(s) with kept originals again: {r['changed']} changed")
        return 0
    if a.action == "rules":
        for r in feed.rules():
            sets = ", ".join(f"{k}={str(r[k]).lower()}" for k in feed.SET_KEYS if k in r)
            when = ", ".join(f"{k}={v}" for k, v in r["when"].items()) or "any event"
            say(f"{'built-in' if r['builtin'] else 'yours   '}  {r['name']:<16} when {when} -> {sets}")
        say("add your own (they run first): careerbrew data edit work/feed-rules.yaml, for example:")
        say("  rules: [{name: alerts, when: {source: slack, channel: 'alerts-*'}, triage: noise}, {name: family, when: {text: [dentist]}, personal: true}]")
        say("then: careerbrew work feed retriage")
        return 0
    if a.action == "open":
        if not may_show_business():
            say("hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
            return 0
        row = next((r for r in feed.load() if r["id"] == a.id), None)
        if row is None:
            say(f"no feed event {a.id} (see: careerbrew work feed show --show)")
            return 1
        say(f"{row['at']} [{row['triage']} by {row['rule']}] {row['source']} {row['title']}" + (" (personal)" if row.get("personal") else ""))
        orig = feed.originals().get(a.id)
        say(orig["text"] if orig and orig.get("text") else "original no longer kept (older than feed_keep_days) or it had no text")
        return 0
    rows = feed.load()
    if not rows:
        say("no events yet: careerbrew connect slack | pagerduty | outlook, then careerbrew work feed pull (plugins: careerbrew work feed add <file.jsonl> --plugin <name>)")
        return 0
    s = feed.summary(rows)
    say(
        f"work feed: {s['events']} event(s): "
        + ", ".join(f"{t}={n}" for t, n in s["triage"].items())
        + f"; open asks={s['open_asks']} ({s['open_asks_overdue']} waiting over {s['overdue_hours']} work hours); evidence={s['evidence']}; personal={s['personal']} (kept out of evidence, the day log, project memory, and the PM update)"
    )
    say("by source: " + ", ".join(f"{k}={v}" for k, v in sorted(s["sources"].items())))
    if not a.show:
        say("titles: careerbrew work feed show --show [--open] [--triage act] (to your terminal)")
        return 0
    if not may_show_business():
        say("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
        return 0
    pick = feed.open_asks(rows, include_personal=a.personal) if a.open else rows
    pick = [r for r in pick if (a.personal or not r.get("personal")) and (not a.triage or r["triage"] == a.triage) and (not a.source or r["source"] == a.source)]
    for r in pick[-a.top :]:
        say(f"- {r['at'][:16].replace('T', ' ')} [{r['triage']}] {r['source']:<9} {r['title']}  ({r['id']})" + (" personal" if r.get("personal") else ""))
    return 0


def cmd_work_live(a: argparse.Namespace) -> int:
    """The live feed (issue #267): start, one cycle, status, stop, over the source adapters in work/live-sources.yaml or a plugin."""
    from .work import listen, live
    from .work.feed import load as feed_rows

    show = may_show_business()
    if a.action == "start":
        text = ""
        if a.sources:
            text = sys.stdin.read() if a.sources == "-" else Path(a.sources).expanduser().read_text(encoding="utf-8")
        r = live.start(text)
        say(f"live feed running: {len(r['sources'])} source(s), {sum(1 for s in r['sources'] if s['required'])} required; each cycle: careerbrew work live cycle")
        for s in r["sources"]:
            where = f"inbox {s['inbox']}" if s["adapter"] == "file" else f"connector {s['connector']}"
            pull = f"; pull {json.dumps(s['pull'], sort_keys=True)}" if s["pull"] and show else ""
            say(f"  {s['name']:<10} {s['kind']:<8} {'required' if s['required'] else 'optional'}  {where}; since {s['since'] or 'the start'}{pull}  ({s['origin']})")
        if not show and any(s["pull"] for s in r["sources"]):
            say("pull hosts and channels hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
        return 0
    if a.action == "stop":
        r = live.stop()
        say(f"live feed stopped after {r['cycles']} cycle(s); cursors kept, so the next start continues" if r["was_running"] else "the live feed was not running")
        return 0
    if a.action == "cycle":
        rec = live.cycle(read_allowed=show)
        stored = sum(r["new"] + r["updated"] for r in rec["sources"].values())
        c = rec["counts"]
        say(f"cycle {rec['n']} {rec['status']}: {stored} event(s) stored; verdicts: " + ", ".join(f"{k}={c[k]}" for k in live.VERDICTS if c[k]) + ("" if any(c.values()) else "none"))
        for name, r in rec["sources"].items():
            err = r["error"] if show or r.get("error_kind") in ("LiveError", "FeedError") else f"{r.get('error_kind', 'error')} (details hidden: stdout is not a terminal)"
            state = f"error: {err}" if r["error"] else f"{r['items']} item(s), {r['new']} new, {r['updated']} updated, {r['skipped']} skipped" if r["pulled"] else "no new intake" if r["unchanged"] else "no fresh reply"
            say(f"  {name:<10} {state}; {r['health']}" + ("; unchanged" if r["unchanged"] else ""))
        titles = {r["id"]: r["title"] for r in feed_rows()} if show and rec["followups"] else {}
        for n, f in enumerate(rec["followups"], 1):  # issue #266: per new event, its class and the work done (drafts only)
            say(f"  event {n:<3} {f['class']:<13} {f['verdict']:<8} {f['source']}: {'; '.join(f['actions'])}" + (f"  ({titles[f['id']]})" if f["id"] in titles else ""))
        for g in rec["goals"]:
            say(f"  {listen.goal_line(g, show)}")
        if rec.get("standup"):
            say(f"  {live.standup_line(rec['standup'])}")
        for s in rec["not_stored"]:
            say(f"not_stored  {s}: a required source is not healthy; the cycle failed")
        for n, act in enumerate(rec["actions"], 1):
            say(f"next {n}. {act}")
        return 1 if rec["not_stored"] else 0
    st = live.status()
    if a.json:
        print(json.dumps(st, indent=2, sort_keys=True))
        return 0
    last = st["last"]
    say(f"live feed: {'running' if st['running'] else 'stopped'}; {st['cycles']} cycle(s)" + (f"; last {last['n']} {last['status']} at {last['at']}" if last else ""))
    h = st["health"]
    say(f"source health: {h['status']}; {h['healthy']} of {h['required']} required source(s); {h['mode']} coverage")
    for command in h["commands"]:
        say(f"  fix: {command}")
    if last.get("not_stored"):
        say(f"  not_stored: {', '.join(last['not_stored'])}")
    for s in st["sources"]:
        say(f"  {s['name']:<10} {s['kind']:<8} {'required' if s['required'] else 'optional'}  {s['adapter']}; since {s['since'] or '-'}; last cycle {'stored' if s['last_stored'] else 'stored nothing'}  ({s['origin']})")
    if not st["sources"] and h["status"] == "missing":
        say("no live sources: careerbrew work live start --sources <file.yaml> (or a plugin with live sources)")
    for n, act in enumerate(st["actions"], 1):
        say(f"next {n}. {act}")
    return 0


def cmd_work_team(a: argparse.Namespace) -> int:
    """Your team roster (issue #180): teammates' events in the work feed are worth a look, never noise."""
    from .work import feed, team

    if a.action == "add":
        row = team.add(a.name, a.chat or "", a.email or "", a.login or "")
        ids = ", ".join(k for k in team.FIELDS if row[k]) or "a name only"
        moved = f"feed events retriaged: {feed.retriage()['changed']} changed" if row["changed"] else "no change"
        say(f"on your team: {row['name'] if may_show_business() else 'a teammate'} ({ids}); {moved}")
        return 0
    if a.action == "remove":
        team.remove(a.name)
        say(f"removed from your team; feed events retriaged: {feed.retriage()['changed']} changed")
        return 0
    if a.action == "feed":
        rows = team.feed(a.days)
        if not rows:
            say(f'no kept teammate events in the last {a.days} day(s) (add teammates: careerbrew work team add "<name>" --chat <id>)')
            return 0
        show = may_show_business()
        loose = sum(not r["projects"] for r in rows)
        say(
            f"{len(rows)} teammate event(s) in the last {a.days} day(s): {len(rows) - loose} match a project (with project memory sync on, their notes go to its memory file), {loose} match none; {sum(len(r['to_research']) for r in rows)} link(s) to research"
        )
        if not show:
            say("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
            return 0
        for r in rows[: a.top]:
            where = ", ".join(r["projects"]) or "no project"
            say(f"- {r['at'][:16].replace('T', ' ')} {', '.join(r['team'])}: {r['title']}  [{where}]  ({r['id']})")
            for e in r.get("research") or []:
                say(f"    {e['url']}: {e['summary'] if e['opened'] else 'not opened: ' + e['reason']}")
            for u in r["to_research"]:
                say(f"    {u}: not researched yet")
        return 0
    rows = team.load()
    show = may_show_business()
    if a.json:
        say(json.dumps([r if show else {k: bool(v) for k, v in r.items()} for r in rows], indent=2))
        return 0
    if not rows:
        say('no teammates yet: careerbrew work team add "Sam Lee" --chat U0SAM --login samlee --email sam@example.com')
        return 0
    say(f"{len(rows)} teammate(s)")
    for r in rows:
        ids = ", ".join(f"{k} {r[k]}" if show else k for k in team.FIELDS if r[k]) or "name only"
        say(f"- {r['name'] if show else '(name hidden)'}: {ids}")
    if not show:
        say("names hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
    return 0


def cmd_work_standup(a: argparse.Namespace) -> int:
    """What you did on each row you report on since the last standup (issue #110): counts and the page path; titles only with --show."""
    from . import config
    from .work import standup

    inputs = {k: getattr(a, k) for k in ("rows", "talk", "extra", "proof") if getattr(a, k)}
    if sum(v == "-" for v in inputs.values()) > 1:
        say("only one of --rows, --talk, --extra, and --proof can read stdin (-); pass the others as files")
        return 2
    texts = {k: sys.stdin.read() if v == "-" else Path(v).expanduser().read_text(encoding="utf-8") for k, v in inputs.items()}
    if "rows" in texts:
        standup.save_rows(texts["rows"])
    if {"talk", "extra", "proof"} & texts.keys():
        n = standup.save_notes(talk=texts.get("talk"), extra=texts.get("extra"), proof=texts.get("proof"))
        talked, extra = len(n["talk"]), len(n["extra"])
        proofs = "" if n["proof"] is None else f", {len(n['proof'])} proof check{'' if len(n['proof']) == 1 else 's'}"
        say(f"agent notes: {talked} talking point{'' if talked == 1 else 's'}, {extra} extra action{'' if extra == 1 else 's'}{proofs} (kept until the next standup)")
    r = standup.run()
    w, c = r["window"], r["counts"]
    say(f"standup since {w['since']} ({w['tz']}), until {w['until']}; compares against the {w['compares']} standup (standup_window {w['mode']})")
    if r["has_rows"]:
        say(f"rows: {c['shown']} shown ({c['yours']} yours, {c['matched']} matched by your work), {c['hidden']} with no update hidden" + (f", {r['updated']}" if r["updated"] else ""))
    if c["gated"]:
        say(f"proof check: {c['passed']} passed, {c['failed']} failed (failed rows stay off the page; standup_proof_minutes {config.get('standup_proof_minutes')})")
        named = a.show and may_show_business()
        for f in r["failed"]:
            title = f" {f['title']}" if named else ""
            reason = f" (agent: {f['reason']})" if named and f["reason"] else ""
            say(f"  failed row {f['row']}{title}: {'; '.join(f['reasons'])}{reason}")
    if standup.no_row_passed(r):
        say("no row passed the proof check" + (f" ({r['updated']})" if r["updated"] else ""))
    elif not r["has_rows"]:
        say("no rows to report on yet: a plugin or an agent passes them with careerbrew work standup --rows <file.json|-> (title, status, owner, keys; see docs/plugins.md)")
    where = "".join(f", {n} in {name}" for name, n in c["by_group"])
    say(f"your actions: {c['actions']} ({c['on_rows']} on rows{where})")
    say(f"page: {r['page']} (or the Standup view in the web app)")
    if not a.show:
        return 0
    if not may_show_business():
        say("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
        return 0
    for row in r["rows"]:
        say(f"- {row['title']}" + (f" [{row['status']}]" if row["status"] else "") + f": {row['talking_point']}")
    for g in r["groups"]:
        say(f"- {g['label']}: {g['talking_point']}")
    return 0


def cmd_work_review_ask(a: argparse.Namespace) -> int:
    """Issue #270: a review ask that names enough people for the PR's required approvals; counts to agents, the draft only where business data may show."""
    from .connectors import ConnectorError
    from .secrets import SecretError
    from .work import review_ask

    try:
        if a.check:
            text = sys.stdin.read() if a.check == "-" else Path(a.check).expanduser().read_text(encoding="utf-8")
            r = review_ask.check_pr(a.url, text)
            if not r["ok"]:
                say(f"refused: the draft names {r['people']} person(s) and {r['groups']} group handle(s); this PR needs {r['needed']} named people (group handles do not count) (breaks US-90)")
                return 1
            say(f"ok: the draft names {r['people']} person(s) for {r['needed']} required approvals")
            return 0
        r = review_ask.build(a.url, a.reviewer)
    except (review_ask.ReviewAskError, ConnectorError, SecretError, OSError, ValueError) as e:
        say(f"error: {e} (breaks US-90)")
        return 2
    src = f"{r['required']} required by {r['source']}" if r["source"] != "unknown" else "required count unreadable"
    say(f"review ask: {r['status']}; needs {r['needed']} approvals ({src}; never fewer than {review_ask.MIN_PEOPLE}); names {len(r['people'])} person(s); {len(r['groups'])} group handle(s) not counted; {r['files']} changed file(s)")
    if r["status"] == "short":
        say(f"refused: {r['needed'] - len(r['people'])} more person(s) needed (fix: add --reviewer <login> per person) (breaks US-90)")
    if may_show_business():
        sys.stdout.write(r["text"])
    else:
        say("draft hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
    return 0 if r["status"] == "ready" else 1


def _say_prep(name: str, r: dict[str, Any], intro: bool = False) -> None:
    """A prep project's readiness gate (issue #276): one line per gate, the open ones with the command that closes them."""
    if intro:
        say(f"{name} is a prep project: it is done only when these {r['total']} gates pass")
    when = f", event on {r['event_on']}" + ("" if r["ready"] else " (that day's plan starts with the open gates)") if r["event_on"] else ""
    say(f"{name}: {'READY' if r['ready'] else 'not ready'}: {r['done']} of {r['total']} gates pass{when}")
    for g in r["gates"]:
        note = f" ({g['note']})" if g["note"] else ""
        say(f"  {'done' if g['done'] else 'open'}  {g['gate']:<9} {g['text']}{note}" + (f"\n        fix: {g['fix']}" if g["fix"] else ""))


def cmd_work_project(a: argparse.Namespace) -> int:
    from .work import projects

    if a.action == "add":
        projects.add(a.name, a.memory or "", a.stale_after, a.title or "", a.group or "", a.parent or "", a.link, a.kind or "", a.event_on or "")
        say(f"project {a.name} added (stale after {projects.parse_days(a.stale_after)}d)")
        if a.kind == "prep":
            _say_prep(a.name, projects.show(a.name)["prep"], intro=True)
    elif a.action == "set":
        p = projects.edit(a.name, a.title, a.group, a.parent, a.link, a.unlink, a.kind, a.event_on, a.quiet_until)
        say(f"{a.name}: parent {p.get('parent') or 'none'}, links {', '.join(p.get('links') or []) or 'none'}, group {p.get('group') or 'none'}")
    elif a.action == "import":
        from .work import project_import

        r = project_import.run(a.source, dry_run=a.dry_run)
        say(f"{'dry run: ' if r['dry_run'] else ''}{r['rows']} row(s): {len(r['added'])} added, {len(r['updated'])} updated, {len(r['unchanged'])} unchanged, {r['sources']} tracker source(s) added")
        for key in ("added", "updated"):
            if r[key]:
                say(f"  {key}: {', '.join(r[key])}")
        if r["ignored"]:
            say(f"  ignored columns: {', '.join(r['ignored'])} (import reads {', '.join(project_import.COLUMNS)})")
        say("nothing written: run it again without --dry-run to save" if r["dry_run"] else "see them: careerbrew work project list, or the Work view")
    elif a.action == "source":
        projects.add_source(a.name, a.kind, a.ref)
        say(f"source added to {a.name}: {a.kind} {a.ref}")
    elif a.action == "check" and a.since:
        from .work import research

        if a.status:
            raise projects.ProjectError(f"--since stamps nothing, so it takes no --status (fix: read what changed, then `careerbrew work project check {a.name} <ref> --status ...`)")
        rows = research.changes(a.name, a.ref, read_allowed=may_show_business())
        say(f"{a.name}: what changed since each source's last check (nothing stamped)")
        for r in rows:
            when = f"since {(r['since'] or '')[:16].replace('T', ' ')} UTC"
            detail = {"changed": f"{r['count']} change(s) {when}", "unchanged": when, "never read": "never checked", "unknown": f"why: {r['reason']}", "reference": f"skipped: {r['reason']}"}[r["state"]]
            say(f"  {r['state']:<10} {r['kind']:<8} {r['ref']}  {detail}")
        todo = sum(r["state"] in ("changed", "never read", "unknown") for r in rows)
        tracked = sum(r["state"] != "reference" for r in rows)
        if todo:
            say(f'read these {todo} of {tracked} (changed, never read, unknown), then stamp each one you read: careerbrew work project check {a.name} <ref> --status "<one line>"')
        else:
            say(f"nothing changed in {tracked} source(s): no read needed now")
    elif a.action == "check":
        n = projects.check(a.name, a.ref, a.status or "")
        say(f"checked {n} source(s) of {a.name}")
    elif a.action == "reference":
        projects.set_reference(a.name, a.ref, on=not a.off)
        if a.off:
            say(f"{a.name}: {a.ref} is tracked again: it goes stale after the project's limit (read it, then careerbrew work project check {a.name} {a.ref})")
        else:
            say(f"{a.name}: {a.ref} is a reference now: careerbrew never reads it and it never goes stale")
    elif a.action == "absorb":
        from .work import research

        name, ref = (a.first, a.second) if a.second else (None, a.first)
        r = research.absorb(ref, name, read_allowed=may_show_business())
        say(f"absorbed {r['kind']} {r['ref']} into {r['project']}" + (f": {r['notes']} note(s) written to the project memory" if r["memory"] else ""))
        if r.get("reason"):
            say(f"note: {r['reason']}")
        elif r["read"] and not r["memory"] and r["notes"]:
            say(f"note: no memory path, so the notes were not saved (fix: careerbrew work project memory {r['project']} <path>)")
    elif a.action == "memory":
        projects.set_memory(a.name, a.path)
        say(f"memory for {a.name} set")
    elif a.action == "rules-set":
        if not a.yes:
            say("standing rules were not changed; rerun with --yes to confirm replacement")
            return 1
        projects.set_rules(a.name, "" if a.clear else a.text)
        say("standing rules saved")
    elif a.action == "rules":
        text = projects.rules(a.name)
        if text and not may_show_business():
            say("standing rules are business data; reading them requires a terminal, owner UI, or plugin read permission")
            return 3
        say(json.dumps({"rules": text}) if a.json else text or "no standing rules")
    elif a.action == "sync":
        from .work import memory_sync

        r = memory_sync.run(allowed=may_show_business())
        if not r["enabled"]:
            say("project memory sync is off (turn it on: careerbrew config set project_memory_sync true)")
            return 0
        i, o = r["in"], r["out"]
        say(f"memory in: {i['files']} changed file(s): {i['sources']} new source(s), {i['proposed']} proposed, {i['links']} link(s), {i['status']} status line(s)" + (f", {i['errors']} unreadable" if i["errors"] else ""))
        say(f"memory out: {o['notes']} new note(s) in {o['projects']} project(s)" + (f", {o['removed']} note(s) of personal events removed" if o["removed"] else ""))
        if o["reason"]:
            say(f"note: {o['reason']}")
    elif a.action == "next":
        projects.link_task(a.name, a.task)
        say(f"next step {a.task} linked to {a.name}")
    elif a.action == "explain":
        from .work import explain

        if a.check:
            path = Path(a.check)
            text = store.read_text(path) if path.resolve().is_relative_to(paths.data_root().resolve()) else path.read_text()
            problems = explain.check(text or "", short=a.short, deep=a.deep)
            for p in problems:
                say(f"problem: {p}")
            say(f"explain check: {'pass' if not problems else f'{len(problems)} problem(s)'}")
            events.emit("work.explain", status="ok" if not problems else "fail", problems=len(problems))
            return 0 if not problems else 1
        out = paths.sub("out") / f"explain-{a.name}.md"
        store.write_text(out, explain.scaffold(a.name, short=a.short, deep=a.deep))
        say(f"wrote {out}: fill it from the sources, then: careerbrew work project explain {a.name} --check {out}" + (" --short" if a.short else " --deep" if a.deep else ""))
        events.emit("work.explain", status="ok", problems=-1)
    elif a.action == "prd":
        from .work import prd

        if a.check:
            path = Path(a.check)
            text = store.read_text(path) if path.resolve().is_relative_to(paths.data_root().resolve()) else path.read_text()
            problems = prd.check(text or "")
            for p in problems:
                say(f"problem: {p}")
            say(f"prd check: {'pass' if not problems else f'{len(problems)} problem(s)'}")
            events.emit("work.prd", status="ok" if not problems else "fail", problems=len(problems))
            return 0 if not problems else 1
        if a.slot is not None and not 0 < a.slot <= 520:
            raise SystemExit(f"error: --slot is the plan slot in dev weeks, more than 0 and at most 520 (got {a.slot:g})")
        out = paths.sub("out") / f"prd-{a.name}.md"
        store.write_text(out, prd.scaffold(a.name, slot=a.slot))
        say(f"wrote {out}: fill it from the sources, then: careerbrew work project prd {a.name} --check {out}")
        events.emit("work.prd", status="ok", problems=-1)
    elif a.action == "list" and a.json:
        say(json.dumps(projects.watch_list(active=not a.all, business=may_show_business()), indent=2))
    elif a.action == "list":
        rows = [r for r in projects.list_rows() if a.all or not r["archived"]]
        for r in rows:
            related = "".join(f"  {k} {v}" for k, v in (("group", r["group"]), ("parent", r["parent"]), ("links", ",".join(r["links"]))) if v)
            refs = f"  references {r['references']}" if r["references"] else ""
            done = "  archived" if r["archived"] else ""
            prep = f"  prep ready {r['prep']['done']}/{r['prep']['total']}" if r["prep"] else ""
            quiet = f"  quiet until {r['quiet_until']}" if r["quiet"] else ""
            say(f"{r['name']:<24} sources {r['sources']}  stale {r['stale']}{refs}  oldest check {r['oldest_checked_at'] or 'never'}{related}{prep}{done}{quiet}")
        if not rows:
            archived = not a.all and any(r["archived"] for r in projects.list_rows())
            say("no active projects: careerbrew work project add <name>, or see the archived ones: careerbrew work project list --all" if archived else "no projects yet: careerbrew work project add <name>")
    elif a.action == "gate":
        _say_prep(a.name, projects.gate(a.name, a.gate, undo=a.undo, refs=a.refs))
    elif a.action == "quiz":
        r = projects.quiz(a.name, a.asked, a.missed)
        q = r["quiz"]
        say(f"{a.name}: quiz stored: score {round(q['score'] * 100)}% ({q['asked'] - q['missed']} of {q['asked']}), {q['missed']} miss(es)")
        if q["missed"]:
            say(f"re-quiz the {q['missed']} miss(es), then store it: careerbrew work project quiz {a.name} --asked {q['missed']} --missed <n>")
        _say_prep(a.name, r)
    elif a.action == "archive":
        projects.archive(a.name, force=a.force)
        say(f"{a.name} archived: off the watch list, never stale; its record and notes stay (undo: careerbrew work project unarchive {a.name})")
    elif a.action == "unarchive":
        projects.archive(a.name, on=False)
        say(f"{a.name} is watched again: its sources go stale after their limit")
    elif a.action == "remove":
        if not a.yes:
            say(f"not removed: this deletes the record of {a.name} (its memory file and tasks stay); rerun with --yes to confirm, or keep it with: careerbrew work project archive {a.name}")
            return 2
        p = projects.remove(a.name)
        kept = f"; its memory file stays: {p['memory']}" if p.get("memory") else ""
        from .work import project_priority

        ranked = any(e["name"] == a.name for e in project_priority.entries())
        say(f"{a.name} removed{kept}" + (f" (it is still ranked: careerbrew work priority remove {a.name})" if ranked else ""))
    elif a.action == "update":
        row = projects.record_update(a.name, a.kind, a.text)
        say(f"{a.name}: {row['kind']} update noted at {row['at']} (latest change in the Work view and `work project list --json`)")
    else:
        d = projects.show(a.name)
        show = may_show_business()
        say(f"{d['name']}: stale after {d['stale_after_days']}d")
        if d["title"] and show:
            say(f"  title: {d['title']}")
        for key in ("group", "parent"):
            if d[key]:
                say(f"  {key}: {d[key]}")
        for key, label in (("children", "children"), ("links", "links"), ("linked_from", "linked from")):
            if d[key]:
                say(f"  {label}: {', '.join(d[key])}")
        if d["prep"]:
            _say_prep(d["name"], d["prep"])
        for s in d["sources"]:
            status = f"  {s['status']}" if show and s.get("status") else ""
            who = projects.reader(s["kind"], s["ref"])
            mark = "ref  " if s["reference"] else "STALE" if s["stale"] else "ok   "
            say(f"  {mark}  {s['kind']:<8} {s['ref']}  {_age(s['age_days'])}  read by {who}{status}")
            if s["reference"]:
                say(f"         reference, never stale: {s['reference_reason']} (track it: careerbrew work project reference {d['name']} {s['ref']} --off)")
            elif s["stale"] and who == "agent":  # a plugin chose agent reads: never point at a connector (issue #184)
                say(f'         why stale: the agent reads it (plugin agent_reads) (fix: read it with the agent\'s own tools, then careerbrew work project check {d["name"]} {s["ref"]} --status "<one line>")')
            elif s.get("stale_reason"):
                say(f"         why stale: {s['stale_reason']}")
        for x in d["proposed"]:
            say(f"  proposed: {x['kind']} {x['ref']} (add: careerbrew work project source {d['name']} {x['kind']} {x['ref']})")
        for t in d["tasks"]:
            say(f"  next: {t['id']}" + (f" {t['title']}" if show else ""))
    return 0


def cmd_work_linkedin(a: argparse.Namespace) -> int:
    r = linkedin.build(role=a.role or "", max_bullets=a.max, since=a.since)
    say(f"LinkedIn block: {r['bullets']} bullet(s), {r['chars']}/{linkedin.LIMIT} chars, public metrics {'kept' if r['public_metrics'] else 'removed by policy'}")
    say(f"file: {r['path']}  (careerbrew never posts; paste it into LinkedIn > Experience yourself)")
    if r["suspects"]:
        say("review before posting, these may be internal names: " + ", ".join(r["suspects"]))
        say(f"  add any that are internal to {paths.data_root() / 'sensitive-terms.txt'} and rerun")
    if a.show:
        # Public-safe text (it passed the leak check), so it may print even to an agent.
        say("")
        say(r["text"])
    return 0


def cmd_work_gaps(a: argparse.Namespace) -> int:
    asm = _assessment(a)
    for i, g in enumerate(promo.gaps(asm), 1):
        say(f"{i}. {g['id']:<14} coverage {g['coverage']:.0%}  priority {g['priority']}  -> {g['how_to_close']}")
    return 0


def cmd_work_task_set(a: argparse.Namespace) -> int:
    """Your own grade for a task (issue #185): accepted tasks start at 3/3/1h, and this changes it."""
    if a.impact is None and a.urgency is None and a.hours is None:
        say("nothing to change: pass --impact, --urgency, or --hours")
        return 2
    t = priorities.regrade(a.task, a.impact, a.urgency, a.hours)
    say(f"{t['id']}: impact {t['impact']}, urgency {t['urgency']}, {t['estimate_h']}h (the plan and the ranks use it now)")
    return 0


def cmd_work_impact(a: argparse.Namespace) -> int:
    """Issue #214: record a task's expected impact, then its measured result; the daily update and promo evidence read them."""
    try:
        t = priorities.record_impact(a.task, a.expect, a.who, a.metric, a.result, a.evidence, a.not_sure)
    except (ValueError, KeyError) as e:
        say(f"error: {str(e).strip(chr(39))} (breaks US-87)")
        return 2
    plan, got = t.get("expected_impact") or {}, t.get("measured_impact") or {}
    say(f"{t['id']}: expected impact {'set' if plan else 'not set'} ({', '.join(sorted(plan)) or 'none'}); measured {'yes' if got.get('result') else 'not yet'}, {len(got.get('evidence', []))} evidence link(s)")
    if got.get("result"):
        say("the daily update and promo evidence use the result now")
    return 0


def cmd_work_task(a: argparse.Namespace) -> int:
    """The owner (a terminal or the web app) adds a task directly; anything else is an agent and proposes it (issue #46)."""
    owner = sys.stdin.isatty() or _owner_ui() or os.environ.get("CAREERBREW_TEST_OWNER") == "1"
    source = a.source or ("user" if owner else "agent")
    if source == "user" and not owner:
        say('only you add tasks directly: agents pass --source agent --why "..." and the task waits in your inbox')
        return 3
    given = a.link or []
    for value in given:  # issue #143: the web app links only a full http(s) address; anything else shows as text
        if not priorities.is_web(priorities.parse_link(value)["url"]):
            say(f"warning: --link {value!r} is not a full http(s):// URL, so the web app shows it as text, not a link (fix: pass the whole address, like https://..., or LABEL=https://...)")
    if len(given) > priorities.MAX_LINKS:
        say(f"warning: a task keeps {priorities.MAX_LINKS} links; the last {len(given) - priorities.MAX_LINKS} were left out")
    t = priorities.add(a.title, a.impact, a.urgency, a.hours, a.competencies.split(",") if a.competencies else [], a.due, source=source, why=a.why or "", links=given)
    if t["status"] == "proposed":
        from .day import inbox

        receipt = inbox.offer_task(t)
        if receipt["status"] == "decided":
            say(f"proposed {t['id']} (added by {source}): inbox {receipt['id']} was already decided")
        else:
            say(f"proposed {t['id']} (added by {source}): it waits in the inbox until the owner accepts it (inbox {receipt['id']}; owner: careerbrew day accept {receipt['id']})")
    else:
        say(f"added {t['id']}")
    return 0


def cmd_work_plan(a: argparse.Namespace) -> int:
    asm = _assessment(a)
    gp = {g["id"]: g["priority"] for g in promo.gaps(asm)}
    plan = priorities.plan(gp, a.hours)
    say(f"plan: {plan['planned_h']:g}h of {plan['capacity_h']:g}h")
    for t in plan["today"]:
        title = t["title"] if may_show_business() else "(title hidden; run in your terminal)"
        say(f"- {t['id']} score {t['score']} promo+{t['promo_bonus']} est {priorities.estimate(t):g}h  {title}")
    return 0


def cmd_work_done(a: argparse.Namespace) -> int:
    t = priorities.done(a.id, a.hours)
    say(f"done {t['id']} ({t['actual_h']}h vs {t['estimate_h']}h planned)")
    return 0


def cmd_ladder_list(_a: argparse.Namespace) -> int:
    for lid, lad in sorted(promo.ladders().items()):
        say(f"{lid:<20} levels={','.join(lad['levels'])} origin={lad['origin']}")
    return 0


def cmd_ladder_import(a: argparse.Namespace) -> int:
    say(f"imported ladder {promo.import_ladder(Path(a.file))}")
    return 0


def cmd_ladder_levels(a: argparse.Namespace) -> int:
    from .work import ladder_grade

    lid, lvl = ladder_grade.target()
    lid, lvl = a.ladder or lid, a.level or lvl
    try:
        dims = ladder_grade.levels(lid, lvl)
    except promo.LadderError as e:
        say(f"error: {e} (breaks US-88)")
        return 2
    if a.json:
        print(json.dumps({"ladder": lid, "level": lvl, "dimensions": dims}, indent=2))
        return 0
    for d in dims:
        say(f"{d['id']:<14} {d['name']}" + (f" — {d['description']}" if d["description"] else ""))
    return 0


def cmd_day_pm_sent(a: argparse.Namespace) -> int:
    """Record the PM update the owner sent so the next draft learns from it (issue #238). Never prints the text."""
    from . import calendar
    from .day import pm_learn

    day = a.day or calendar.now().date().isoformat()
    text = Path(a.file).expanduser().read_text(encoding="utf-8")
    if not store.read_json(pm_learn.shown_path(day), {}):
        say(f"no draft was shown for {day}: show the PM update first (Day log in the web app, or `careerbrew day send` without --yes)")
        return 0
    pm_learn.save_sent(day, text)
    learned = pm_learn.learn_sent(day)
    say(("learned from the sent update" if learned else "the sent update is already learned") + f" for {day}")
    return 0


def cmd_day_pm_grade(a: argparse.Namespace) -> int:
    """Grade today's PM update against the next level; the owner sees lines and hints, agents only coverage."""
    from .day import log as daylog
    from .work import ladder_grade

    d = daylog.save()
    lid, lvl = ladder_grade.target()
    try:
        g = ladder_grade.grade(daylog.pm_report(d)["body"].splitlines(), lid, lvl)
    except promo.LadderError as e:
        say(f"error: {e} (breaks US-88)")
        return 2
    added = ladder_grade.record(g, d["day"]) if a.record else 0
    owner = may_show_business()
    if not owner:  # line text is business content
        g = {
            **g,
            "lines": [{"dims": t["dims"]} for t in g["lines"]],
            "coverage": [{k: v for k, v in r.items() if k != "lines"} | {"line_count": len(r["lines"])} for r in g["coverage"]],
            "suggestions": [{"id": h["id"]} for h in g["suggestions"]],
        }
    if a.json:
        print(json.dumps({**g, "recorded": added}, indent=2))
        return 0
    say(f"ladder {lid}, next level {lvl} (for you only: never part of the sent update)")
    for r in g["coverage"]:
        say(f"  {r['coverage']:<6} {r['name']} ({len(r['lines']) if owner else r['line_count']} line(s))")
    if owner:
        for h in g["suggestions"]:
            say(f"  hint {h['id']}: " + (f'"{h["line"]}": ' if h["line"] else "") + h["hint"])
    if a.record:
        say(f"recorded {added} tagged line(s) as evidence")
    return 0


# ---------- data at rest ----------
def cmd_data_status(_a: argparse.Namespace) -> int:
    st = vault.status()
    say(f"encryption at rest: {'on' if st['enabled'] else 'off'} · {st['sealed']} of {st['files']} file(s) encrypted · key: {backup.key_path()}")
    if st["enabled"] and st["plaintext"]:
        say(f"{st['plaintext']} file(s) still plaintext: run `careerbrew data encrypt`")
    return 0


def cmd_data_encrypt(_a: argparse.Namespace) -> int:
    n = vault.encrypt_all()
    events.emit("data.encrypt", status="on", items=n)
    say(f"encrypted {n} file(s); encryption at rest is on. Keep the key ({backup.key_path()}) in your password manager: without it the data cannot be opened")
    return 0


def cmd_data_decrypt(a: argparse.Namespace) -> int:
    if not a.yes:
        say("this writes every data file as plaintext and turns encryption off; rerun with --yes to confirm")
        return 1
    n = vault.decrypt_all()
    events.emit("data.encrypt", status="off", items=n)
    say(f"decrypted {n} file(s); encryption at rest is off (`careerbrew data encrypt` turns it back on)")
    return 0


def cmd_data_set(a: argparse.Namespace) -> int:
    if a.file != "profile.yaml":
        print("data set supports only profile.yaml")
        events.emit("data.set", status="fail")
        return 1
    fields = {name: value for name in ("name", "employer") if (value := getattr(a, name)) is not None}
    try:
        changed = machine.set_profile_fields(fields)
    except (OSError, UnicodeError, ValueError, RuntimeError):
        # This fixed message contains no data; the normal redactor reads the profile that may be broken.
        print("profile update failed; check the profile and encryption key, then use the editor if needed")
        events.emit("data.set", status="fail")
        return 1
    events.emit("data.set", status="ok", items=len(changed))
    # `changed` contains only the fixed field names; the profile redactor may be unavailable.
    print(f"saved profile.yaml fields: {', '.join(changed)}")
    return 0


def _personal_data(rel: Path) -> bool:
    """True when the data file at `rel` holds personal data only: a file at the top of the data root, or one
    in a job-hunt folder (`machine.PERSONAL_DIRS`). Its text may print to an agent.

    An allowlist, so the gate fails closed: work/, day/, ladders/, cache/ (work git evidence), plugins/,
    out/, any new folder, and a folder named in another case (macOS opens WORK/ as work/) count as business.
    """
    return len(rel.parts) == 1 or rel.parts[0] in machine.PERSONAL_DIRS


def _data_file(name: str) -> Path | None:
    """The data file `name` names (relative to the data root), or None when it is outside the root or not a file."""
    root = paths.data_root().resolve()
    target = (root / name).resolve()
    if not target.is_relative_to(root) or not target.is_file():
        say(f"no such data file: {name} (paths are relative to the data root, e.g. profile.yaml)")
        return None
    return target


def cmd_data_show(a: argparse.Namespace) -> int:
    """Print one data file decrypted (redacted), or with --keys only its structure. It writes nothing (issue #103)."""
    target = _data_file(a.file.removeprefix("private/"))
    if target is None:
        return 1
    rel = target.relative_to(paths.data_root().resolve())
    hidden = not _personal_data(rel) and not may_show_business()
    try:
        if a.keys:
            o = store.outline(target, strict=hidden)
            unit = "record(s)" if o["kind"] == "jsonl" else "line(s)"
            note = " (no keys: not YAML, JSON, or JSONL)" if o["kind"] == "text" else ""
            say(f"{rel.as_posix()}: {o['kind']}, {'encrypted' if o['sealed'] else 'plaintext'} on disk, {o['count']} {unit}{note}")
            for line in o["keys"]:
                say(line)
            events.emit("data.show", status="keys", items=len(o["keys"]))
            return 0
        if hidden:
            say(
                f"{rel.as_posix()} may hold business data: its text prints only to your own terminal or the web app, or when a plugin policy sets agent_may_read_business."
                f" Agents can read files at the top of the data root and in {', '.join(f'{d}/' for d in machine.PERSONAL_DIRS)}. For its structure: careerbrew data show {rel.as_posix()} --keys"
            )
            events.emit("data.show", status="hidden")
            return 3
        text = store.read_text(target) or ""
    except UnicodeDecodeError:
        say(f"{rel.as_posix()} is not a text file")
        return 1
    except ValueError as e:  # store.outline names the line, never the text
        say(f"{rel.as_posix()}: {e}")
        return 1
    say(text.rstrip("\n"))
    events.emit("data.show", status="text", items=len(text.splitlines()))
    return 0


def cmd_data_edit(a: argparse.Namespace) -> int:
    """Open a data file in $EDITOR as plaintext, then encrypt it again. The plaintext copy lives in the data root, mode 600, and is deleted after."""
    import shlex
    import subprocess

    import yaml

    root = paths.data_root()
    target = _data_file(a.file)
    if target is None:
        return 1
    if not sys.stdin.isatty():
        say("data edit needs a terminal: it opens your $EDITOR")
        return 1
    tmp = root / ".edit" / target.name
    tmp.parent.mkdir(mode=0o700, exist_ok=True)
    try:
        tmp.write_text(store.read_text(target) or "", encoding="utf-8")
        tmp.chmod(0o600)
        subprocess.run([*shlex.split(os.environ.get("EDITOR", "vi")), str(tmp)], check=True)  # noqa: S603 (the user's own editor)
        text = tmp.read_text(encoding="utf-8")
        if target.suffix in (".yaml", ".yml"):
            yaml.safe_load(text)
        elif target.suffix == ".json":
            json.loads(text)
        store.write_text(target, text)
    finally:
        tmp.unlink(missing_ok=True)
    say(f"saved {a.file} (encrypted)" if vault.enabled() else f"saved {a.file}")
    return 0


# ---------- backup / plugin / privacy / review ----------
def cmd_backup_create(_a: argparse.Namespace) -> int:
    for p in backup.create():
        say(f"backup written and verified: {p}")
    return 0


def cmd_backup_verify(a: argparse.Namespace) -> int:
    p = Path(a.file) if a.file else backup.latest()
    if p is None:
        say("no backup found (run `careerbrew backup create`)")
        return 1
    say(f"ok: {p.name} decrypts, hash matches, {backup.verify(p)} file(s)")
    return 0


def cmd_backup_restore(a: argparse.Namespace) -> int:
    n = backup.restore(Path(a.file), Path(a.dest))
    say(f"restored {n} file(s) to {a.dest}")
    say("the machine role is not part of a backup: a folder restored on a corporate machine is marked corporate; check with `CAREERBREW_HOME=<folder> careerbrew machine show`")
    return 0


def cmd_plugin_add(a: argparse.Namespace) -> int:
    e = plugins.add(a.source, allow_exec=a.allow_exec)
    say(f"added plugin {e['name']} {e['version']} ({e['origin']}), exec_allowed={e['exec_allowed']}")
    return 0


def cmd_plugin_refresh(_a: argparse.Namespace) -> int:
    names = plugins.refresh()
    say(f"refreshed {len(names)} plugin(s): {', '.join(names) or 'none installed'}")
    return 0


def cmd_plugin_remove(a: argparse.Namespace) -> int:
    r = plugins.remove(a.name, purge_data=a.purge_data)
    say(f"removed plugin {a.name}; purged {r['purged_records']} record(s)")
    return 0


def cmd_plugin_list(_a: argparse.Namespace) -> int:
    ps = plugins.installed()
    if not ps:
        say("no plugins installed")
    for p in ps:
        provides = ",".join(sorted((p["manifest"].get("provides") or {}).keys()))
        say(f"{p['name']:<16} {p['version']:<8} provides={provides} exec={p['exec_allowed']}")
    pol = plugins.effective_policy()
    say(f"effective policy: {json.dumps(pol)}")
    return 0


def cmd_plugin_verify(_a: argparse.Namespace) -> int:
    bad = 0
    for name, ok in plugins.verify():
        say(f"{'ok' if ok else 'TAMPERED'}  {name}")
        bad += not ok
    return 1 if bad else 0


def to_terminal(text: str) -> None:
    """Write to the owner's terminal only (`/dev/tty`), never stdout, so an agent's pipe cannot read a term."""
    try:
        with open("/dev/tty", "w") as tty:
            tty.write(text + "\n")
    except OSError as e:
        raise ValueError("--show prints to your own terminal only; run it there, not through an agent") from e


def cmd_privacy_scan(a: argparse.Namespace) -> int:
    if a.files and (a.staged or a.repo):
        raise ValueError("--file scans the files you name only; drop --staged and --repo")
    if a.files:
        rep = privacy.scan_files([Path(f).expanduser() for f in a.files])
    else:
        rep = privacy.scan_repo(Path(a.repo) if a.repo else None, staged_only=a.staged)
    for path, why in rep.findings:
        spans = rep.spans.get((path, why))
        if not spans:
            say(f"LEAK  {path}: {why}")
            continue
        for line, how, got in spans:
            say(f"LEAK  {path}:{line}: {why} ({how})")
            if a.show:
                to_terminal(f"      line {line}: {got}")
    for path, note in rep.notes:
        say(f"NOTE  {path}: {note}")
    say(f"privacy scan: {rep.files_scanned} file(s), {len(rep.findings)} finding(s)")
    if rep.spans and not a.show:
        say("see each matched text yourself: add --show (prints the term; never paste it into chat or an issue)")
    return 0 if rep.ok else 1


def cmd_review(a: argparse.Namespace) -> int:
    from . import report, runs

    step = run_say()
    rows = review.run()
    for r in rows:
        step(f"{r['status']:<8} {r['metric']:<24} {r['value']!s:<8} {r['action']}")
    out = None
    if a.html:
        out = paths.sub("out", "dashboard.html")
        out.write_text(review.dashboard(rows), encoding="utf-8")
        step(f"dashboard: {out}")
    report.finish(runs.review(rows, out), say)
    return 0


# ---------- connectors / machines / sync ----------
def cmd_connect(a: argparse.Namespace) -> int:
    """Guided connect: where to get the token, the missing fields, a "who am I" check, then save.

    The token goes to the Keychain (or stays where `--ref` points). A token that fails the check is not saved.
    """
    allowed = plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and a.kind not in allowed:  # before any prompt, token read, or sign-in call
        say(f"connector {a.kind} is not allowed by plugin policy (allowed: {allowed})")
        return 3
    if a.kind == "outlook" and not a.ref and not getattr(a, "paste", False):
        return _connect_outlook_device(a)
    guide = connectors.GUIDE[a.kind]
    interactive = sys.stdin.isatty()
    defaults = connectors.plugin_defaults(a.kind)
    api = getattr(a, "api", None) or defaults.get("api")
    say(f"1. Create a {a.kind} token: {connectors.token_page(a.kind, api, defaults)}")
    say(f"   It looks like {guide['looks_like']}. Scope: {guide['scope']}.")
    cfg: dict[str, Any] = {"kind": a.kind}
    for key in ("base_url", "email", "user", "api", "days"):
        val = getattr(a, key, None) or defaults.get(key)  # flags win; a company plugin's connector_defaults fill the rest
        if val:
            cfg[key] = val
    for need in guide["needs"]:
        if not cfg.get(need):
            if not interactive:
                say(f"missing --{need.replace('_', '-')} (fix: rerun with it, e.g. --{need.replace('_', '-')} {'https://your-site.atlassian.net' if need == 'base_url' else 'you@example.com' if need == 'email' else 'your-login'})")
                return 2
            cfg[need] = input(f"2. {need.replace('_', ' ')}: ").strip()
    if a.ref:
        ref = a.ref
        secrets.backend_of(ref)
        token = secrets.resolve(ref)
    else:
        if not interactive:
            say("refusing to read a token from a pipe; run in your terminal, or pass --ref keychain:/op:/env:")
            return 3
        token = getpass.getpass(f"3. Paste the {a.kind} token (input hidden): ").strip()
        ref = ""
    if not a.no_verify:
        res = connectors.verify(cfg, token)
        say(("ok    " if res["ok"] else "FAIL  ") + res["message"])
        if not res["ok"]:
            return 1
        if res.get("base_url"):
            cfg["base_url"] = res["base_url"]
    if not ref:
        ref = secrets.store_keychain(f"careerbrew.{a.kind}", token)
    cfg["token"] = ref
    _save_connector(cfg)
    return 0


def _save_connector(cfg: dict[str, Any]) -> None:
    path = paths.sub("work", "connectors.yaml")
    data = store.read_yaml(path, {"connectors": []}) or {"connectors": []}
    data["connectors"] = [c for c in data.get("connectors", []) if c.get("kind") != cfg["kind"]] + [cfg]
    store.write_yaml(path, data)
    say(f"connector {cfg['kind']} saved with secret reference {secrets.backend_of(cfg['token'])} (no token stored in files)")


def _connect_outlook_device(a: argparse.Namespace) -> int:
    """Outlook sign-in with a device code (issue #199): open a link, type a code; the refresh token goes to the Keychain."""
    from . import actions, msauth

    client_id, tenant = msauth.settings(getattr(a, "client_id", None), getattr(a, "tenant", None))
    manual = "A manual token instead: careerbrew connect outlook --paste (only a token your admin issued or allows; it expires after about an hour)."
    if not client_id:
        say("outlook sign-in needs your company's approved app registration (its client ID). Fix: careerbrew config set outlook_client_id <client-id> (a company plugin can set connector_defaults.outlook.client_id).")
        say(manual)
        return 2
    if actions.OWNER_UI.get():
        say("Outlook sign-in waits for you to type a code: run `careerbrew connect outlook` in your terminal")
        return 3
    if not (sys.stdout.isatty() or os.environ.get("CAREERBREW_TEST_OWNER") == "1"):  # the code must not reach a pipe or a log
        say("Outlook sign-in shows a one-time code: run `careerbrew connect outlook` in your own terminal")
        return 3
    cfg: dict[str, Any] = {"kind": "outlook", "auth": "device", "client_id": client_id, "tenant": tenant}
    defaults = connectors.plugin_defaults("outlook")
    for key in ("api", "days"):
        val = getattr(a, key, None) or defaults.get(key)
        if val:
            cfg[key] = val
    try:
        flow = msauth.start(client_id, tenant)
        say(f"1. Open {flow.get('verification_uri', 'https://microsoft.com/devicelogin')} and enter the code {flow['user_code']}")
        say(f"2. Sign in with your work account and accept read-only access ({connectors.GUIDE['outlook']['scope']}). careerbrew waits here for up to {int(flow.get('expires_in') or 900) // 60} minutes.")
        tokens = msauth.poll(client_id, tenant, flow)
    except connectors.ConnectorError as e:
        say(f"FAIL  {e}")
        return 1
    except OSError as e:
        say(f"FAIL  outlook: could not reach Microsoft sign-in ({type(e).__name__}) (fix: check your network and VPN)")
        return 1
    if not a.no_verify:
        res = connectors.verify(cfg, str(tokens["access_token"]))
        say(("ok    " if res["ok"] else "FAIL  ") + res["message"])
        if not res["ok"]:
            return 1
    cfg["token"] = secrets.store_keychain(msauth.REFRESH_SERVICE, str(tokens["refresh_token"]))
    msauth.signed_in(cfg["token"])
    _save_connector(cfg)
    say("careerbrew refreshes the sign-in before each read; if Microsoft ends it, Setup and Today show one step: Reconnect Outlook.")
    return 0


def cmd_connectors_list(_a: argparse.Namespace) -> int:
    data = store.read_yaml(paths.data_root() / "work" / "connectors.yaml", {}) or {}
    pol = plugins.effective_policy()
    for c in data.get("connectors", []):
        allowed = pol["allowed_connectors"] is None or c["kind"] in pol["allowed_connectors"]
        say(f"{c['kind']:<8} secret={secrets.backend_of(c['token'])} allowed_by_policy={allowed}")
    say(f"policy: connectors={pol['allowed_connectors']} hosts={pol['allowed_hosts']} secret_backends={pol['secret_backends']}")
    return 0


def cmd_machine_show(_a: argparse.Namespace) -> int:
    info = machine.info()
    say(f"role: {machine.role()} label: {info.get('label', '-')}")
    hint = machine.suggest()
    if hint and hint != machine.role():
        say(f"plugin signals suggest this is a {hint} machine: `careerbrew machine set {hint}`")
    return 0


def cmd_checkup(a: argparse.Namespace) -> int:
    from . import checkup, report, runs

    step = run_say()
    rows = checkup.run(offline=a.offline)
    for r in rows:
        step(f"{'pass' if r['ok'] else 'FAIL'}  {r['step']}" + (f"  -> {r['fix']}" if r["fix"] else ""))
    bad = sum(not r["ok"] for r in rows)
    step(f"checkup: {len(rows) - bad}/{len(rows)} passed")
    report.finish(runs.checkup(rows, a.offline), say)
    return 1 if bad else 0


def _git_config(key: str) -> str:
    import subprocess

    res = subprocess.run(["git", "config", "--get", key], capture_output=True, text=True, check=False)
    return res.stdout.strip()


def cmd_machine_new_user(a: argparse.Namespace) -> int:
    """One command for a second person at the company: their own settings from the shared plugin (issue #48)."""
    from . import report, runs

    step = run_say()
    if a.plugin and not plugins.installed():
        plugins.add(a.plugin)
    email = a.email or _git_config("user.email")
    name = a.name or _git_config("user.name") or "Your Name"
    terms = machine.plugin_terms()
    r = machine.new_user(Path(a.repos), name, a.employer or (terms[0] if terms else ""), email, a.github_user or "")
    step(f"set up for you with plugin {r['plugin']}: {r['repos']} git repo(s) as evidence sources, connectors {', '.join(r['tools']) or 'none'}")
    for cmd in r["todo"]:
        step(f"  type your token: {cmd}")
    bad = [x for x in r["readiness"] if not x["ok"]]
    for x in bad:
        step(f"FAIL  {x['check']}  -> {x['fix']}")
    step(f"corporate readiness: {len(r['readiness']) - len(bad)}/{len(r['readiness'])}")
    step(f"{runs.BUG_REPORT.label}: `{runs.BUG_REPORT.command}`")
    report.finish(runs.machine_new_user(r), say)
    return 0 if not bad else 1


def cmd_machine_split_plugin_evidence(a: argparse.Namespace) -> int:
    """Move per-person sources and connectors out of the company plugin into your private files, as they are (issue #102).

    Prints counts only, never the moved values."""
    r = machine.split_plugin_evidence(dry_run=a.dry_run)
    verb = "would move" if r["dry_run"] else "moved"
    for p in r["plugins"]:
        if p.get("skipped"):
            say(f"skip  plugin {p['plugin']} {p['file']}: {p['skipped']}")
        else:
            say(f"plugin {p['plugin']} {p['file']}: {p['before']} evidence entries -> {p['after']} shared ({verb} {p['sources']} source(s), {p['connectors']} connector(s))")
    if not any(not p.get("skipped") for p in r["plugins"]):
        say("nothing to move: no plugin folder here holds per-person sources or connectors")
    (s0, s1), (c0, c1), (t0, t1) = r["mine"]["sources"], r["mine"]["connectors"], r["total"]
    say(f"your work/sources.yaml: {s0} -> {s1} source(s); work/connectors.yaml: {c0} -> {c1} connector(s)")
    say(f"evidence sources work collect reads: {t0} -> {t1}")
    if r["dry_run"]:
        say("dry run: nothing written; rerun without --dry-run to move them")
        return 0
    ok = not r["left"]
    say(f"{'ok  ' if ok else 'FAIL'}  {machine.PER_USER_CHECK}" + ("" if ok else f"  -> still in {', '.join(r['left'][:3])}"))
    if any(not p.get("skipped") for p in r["plugins"]):
        say("next: commit the change in the plugin's own repo, so a coworker's copy holds shared entries only")
    return 0 if ok else 1


def cmd_machine_ready(_a: argparse.Namespace) -> int:
    from . import report, runs

    step = run_say()
    rows = machine.readiness()
    for r in rows:
        step(f"{'ok  ' if r['ok'] else 'FAIL'}  {r['check']}" + (f"  -> {r['fix']}" if r["fix"] else ""))
    bad = sum(not r["ok"] for r in rows)
    step(f"corporate readiness: {len(rows) - bad}/{len(rows)}")
    report.finish(runs.machine_ready(rows), say)
    return 1 if bad else 0


def cmd_machine_set(a: argparse.Namespace) -> int:
    d = machine.set_role(a.role, a.label or "")
    say(f"role set: {d['role']}")
    return 0


def cmd_pair_init(_a: argparse.Namespace) -> int:
    if machine.role() == "corporate":
        say("export level: none. careerbrew never copies corporate data to your personal machine; pairing only receives settings (ladder, target level) from your personal machine.")
    key = sync.pair_init()
    if sys.stdout.isatty():
        print(f"pairing key (store it in your password manager, then `careerbrew pair join <key>` on the other machine):\n{key}")
    else:
        say(f"pairing key written to {sync.key_path()} (not printed: stdout is not a terminal). Copy it to your password manager yourself.")
    return 0


def cmd_pair_join(a: argparse.Namespace) -> int:
    key = a.key or (getpass.getpass("pairing key (hidden): ") if sys.stdin.isatty() else sys.stdin.readline().strip())
    sync.pair_join(key)
    say("paired")
    return 0


def cmd_sync_export(a: argparse.Namespace) -> int:
    r = sync.export(Path(a.file))
    say(f"exported from {r['from']} machine: level={r['level']} items={r['items']} -> {a.file}")
    return 0


def cmd_sync_import(a: argparse.Namespace) -> int:
    r = sync.import_bundle(Path(a.file))
    say(f"imported bundle from {r['from']} machine: {r['added']} new record(s)")
    if r.get("ignored"):
        say(f"ignored {r['ignored']} setting(s): only a known ladder and one of its levels are accepted on a corporate machine")
    return 0


# ---------- interview / prep ----------
def cmd_interview(a: argparse.Namespace) -> int:
    from . import interview

    info = interview.loop_for(a.company, a.level)
    text = interview.render(info, with_sources=not a.no_sources)
    out = paths.sub("out", f"interview-{''.join(c if c.isalnum() else '-' for c in a.company.casefold())}.md")
    out.write_text(text, encoding="utf-8")
    say(text if a.show else f"{info['company']}: {len(info['loop'])} step(s) ({'sourced company loop' if info['known'] else 'shared default loop'}); full guide with tips and sources: {out}")
    return 0


def cmd_serve(a: argparse.Namespace) -> int:
    from . import server

    if not server.dist_exists():
        say("the web app is not built yet: cd web && npm ci && npm run build (the API still runs)")
    server.run(a.port)
    return 0


def cmd_web_rotate_token(a: argparse.Namespace) -> int:
    """A new web token (issue #141): every open tab, bookmark, and sign-in cookie stops working; the token never prints."""
    from . import server

    if not a.yes:
        say("this signs out every open web app tab and bookmark (they need a new link from `careerbrew open`); rerun with --yes to confirm")
        return 1
    info = server.rotate_token()
    say(f"web token rotated: open tabs and bookmarks are signed out; run `careerbrew open` for a signed-in tab (the new token lasts {info['days']} days)")
    return 0


def cmd_open(a: argparse.Namespace) -> int:
    """Open the web app in your default browser; the token goes only to the browser, never to stdout."""
    import webbrowser

    from . import config, report, server

    port = int(config.get("serve_port"))
    if a.report:
        s = report.resolve(a.report)
        if not s:
            say(f"no run page {a.report} (see: careerbrew report list)")
            return 1
        page = report.folder() / f"{s['id']}.html"
        # Through the web app (behind its token) when it is built; the page file itself otherwise.
        url = server.app_url(port, f"reports/{s['id']}") if server.dist_exists() else page.as_uri()
        if not webbrowser.open(url):
            say(f"could not open a browser here; open {page} on the machine with your desktop")
            return 1
        say(f"opened run page {s['id']} in your browser ({page})")
        return 0
    url = server.app_url(port, a.view)
    if not webbrowser.open(url):
        say(f"could not open a browser here; run `careerbrew open` on the machine with your desktop (web app: http://127.0.0.1:{port}/)")
        return 1
    say(f"opened the web app in your browser: http://127.0.0.1:{port}/#/{a.view or 'today'}")
    return 0


def cmd_changes(a: argparse.Namespace) -> int:
    from . import changes

    found = changes.check(fetch=a.fetch)
    if not found:
        say("no changes since the last check (the first run only records a baseline)")
        return 0
    for c in found:
        say(f"{c['change']}: {c['kind']} {c['id']} -> {c.get('action', '')}")
    return 0


def cmd_research_page(a: argparse.Namespace) -> int:
    from . import research

    r = research.page(a.url)
    say(f"fetched {r['chars']} chars at {r['fetched_at']}; {'CHANGED since last fetch' if r['changed'] else 'unchanged or first fetch'}; sha256 {r['sha256'][:12]}")
    return 0


def cmd_research_stale(a: argparse.Namespace) -> int:
    from . import research

    rows = research.stale(a.days)
    say(f"{len(rows)} source(s) older than {a.days} days")
    for r in rows[: a.top]:
        flag = " [pipeline]" if r["in_pipeline"] else ""
        say(f"  {r['age_days']}d  {r['id']}{flag}  {r['url']}  ({', '.join(r['companies']) or 'strategy'})")
    if rows:
        say("re-check with the browser-research skill, then rerun scripts/import-research.py")
    return 0


def cmd_interview_patterns(a: argparse.Namespace) -> int:
    from . import interview

    pt = interview.patterns()
    if a.json:
        say(json.dumps(pt, indent=2, default=list))
        return 0
    say(f"shared across {pt['companies']} sourced companies:")
    say("  how many companies use each step: " + ", ".join(f"{s} {v:.0%}" for s, v in pt["step_share"].items()))
    say("  typical order: " + " → ".join(pt["typical_order"]))
    say("  common transitions: " + "; ".join(f"{a_} → {b} ({k})" for a_, b, k in pt["transitions"]))
    say("  AI tools allowed: " + ", ".join(f"{k}={v}" for k, v in sorted(pt["ai_policy"].items())))
    for p in pt["ai_shared_patterns"]:
        say(f"  AI pattern: {p['text']}")
    say(f"  default loop for unknown companies (level 3): {' → '.join(interview.default_loop(3))}")
    return 0


def cmd_prep_check(a: argparse.Namespace) -> int:
    from . import prep

    answers = prep.load_answers(a.answers, a.yes)
    ask = input if (prep.interactive() and not a.json and not answers) else None
    r = prep.run(a.company, answers, ask, a.level)
    if a.json:
        say(json.dumps({k: r[k] for k in ("company", "known_loop", "score", "trend", "gaps")}, indent=2))
        return 0
    say(f"prep for {r['company']}: {r['score']:.0%} ready (trend {' → '.join(f'{x:.0%}' for x in r['trend'])}); {len(r['gaps'])} gap(s)")
    for g in r["gaps"][: a.top]:
        say(f"  [{g['step']}] {g['text']}\n      → {g['action']}")
    say("answer items non-interactively: careerbrew prep check <company> --yes id,id   (ids via --json)")
    return 0


def cmd_prep_story(a: argparse.Namespace) -> int:
    from . import prep

    s = prep.add_story(a.title, [c for c in a.competencies.split(",") if c], a.situation or "", a.action or "", a.result or "")
    say(f"story {s['id']} saved ({len(prep.stories())} in the bank)")
    return 0


# ---------- mail / linkedin ----------
def cmd_mail_scan(a: argparse.Namespace) -> int:
    from . import mail
    from .jobs import linkedin

    msgs, errors = mail.fetch(a.days)
    groups = mail.classify(msgs)
    say(f"mail: {len(msgs)} message(s) from {len(mail.accounts())} account(s) in {a.days} day(s): " + ", ".join(f"{k}={len(v)}" for k, v in groups.items()))
    if machine.role() == "corporate":
        say("corporate machine: job-hunt mail is not processed here")
    else:
        added = linkedin.add_alert_postings(groups["linkedin_alert"])
        say(f"LinkedIn alerts: {added} new posting(s) (see careerbrew jobs list)")
        if groups["interview"]:
            store.append_jsonl(paths.sub("jobs", "inbox.jsonl"), [{"id": m.id, "sender_domain": m.sender.split("@")[-1], "subject": m.subject, "received": m.received} for m in groups["interview"]])
            say(f"{len(groups['interview'])} interview or recruiter email(s): update stages with careerbrew jobs log <company> <tier> <stage>")
    for err in errors:
        say(f"  account error {err}")
    return 0


def cmd_jobs_linkedin_import(a: argparse.Namespace) -> int:
    from .jobs import linkedin

    r = linkedin.import_export(Path(a.zip))
    say(f"LinkedIn export: {r['positions']} position(s), {r['saved_jobs']} saved job(s), {r['applications_logged']} application(s) logged, {r['skills_suggested']} skill(s) to consider adding to profile.yaml")
    return 0


# ---------- plugin wizard / secrets ----------
def cmd_plugin_new(a: argparse.Namespace) -> int:
    from . import report, runs, wizard

    step = run_say()
    opts = {k: v for k, v in {"jira_url": a.jira_url, "email": a.email, "github_user": a.github_user, "github_api": a.github_api}.items() if v}
    tools = [t for t in (a.connect or "").split(",") if t]
    r = wizard.new(a.company, Path(a.dir) if a.dir else None, tools, opts, prompt=not a.no_prompt)
    step(f"plugin {r['plugin']['name']} created at {r['folder']} and installed ({r['plugin']['origin']})")
    step(f"connectors: {', '.join(tools) or 'none'}; policy: local model only, no export off this machine, agents may not read business data")
    for cmd in r["todo"]:
        t = cmd.rsplit(".", 1)[-1]
        g = connectors.GUIDE[t]
        step(f"  {t}: create a token at {g['token_page']} (it looks like {g['looks_like']}), then run: {cmd}")
    if "jira" in tools and not a.email:
        step("  jira also needs your Atlassian login email: rerun with --email you@example.com, or add `email:` to the plugin's evidence.yaml")
    step(f"next: edit {r['folder']}/ladders/*.yaml with the real promotion criteria, then `careerbrew plugin refresh`")
    report.finish(runs.plugin_new(r, tools, "jira" in tools and not a.email), say)
    return 0


def cmd_secret_set(a: argparse.Namespace) -> int:
    if not sys.stdin.isatty():
        say("refusing to read a secret from a pipe; run this in your terminal")
        return 3
    value = getpass.getpass(f"value for {a.service} (input hidden): ")
    if not value:
        say("nothing stored")
        return 1
    say(f"stored as {secrets.store_keychain(a.service, value)}")
    return 0


# ---------- day ----------
def cmd_day_watch(a: argparse.Namespace) -> int:
    from .day import watch

    action = a.action
    if action == "on":
        st = watch.start(interval=a.interval)
        say(f"day watcher on (pid {st.get('pid')}){' — ' + st['note'] if st.get('note') else ''}. Pause any time: careerbrew day watch pause")
    elif action == "off":
        st = watch.stop()
        say(f"day watcher off (still running: {st['running']})")
    elif action == "pause":
        watch.pause()
        say("day watcher paused; it finishes the source in flight, then waits. Resume: careerbrew day watch resume")
    elif action == "resume":
        watch.resume()
        say("day watcher resumed from its last checkpoint")
    elif action == "reload":
        st = watch.reload()
        say("watcher asked to load the current code at the next safe point (state, pause, and results are kept)" if st.get("note") != "not running" else "watcher is not running; `careerbrew day watch on` starts it with the current code")
    elif action == "once":
        s = watch.run_cycle()
        say(f"cycle: {s['added']} new result(s), {s['cpu_ms']} ms CPU, {s['rss_mb']} MB peak memory")
    else:
        st = watch.status()
        last = st.get("last_cycle") or {}
        say(f"state: {st.get('state')}  running: {st['running']}  pid: {st.get('pid')}  code reloads: {st.get('reloads', 0)}")
        if st.get("reload_error"):
            say(f"last update: {st['reload_error']}")
        say(f"last cycle: {last.get('at', '-')}  added {last.get('added', '-')}  cpu {last.get('cpu_ms', '-')} ms  memory {last.get('rss_mb', '-')} MB")
        say("today: " + (", ".join(f"{k}={v}" for k, v in st["today"].items()) or "nothing yet"))
    return 0


NOTIFY_REASONS = {
    "sent": "1 pop-up ({n} together)",
    "scheduled": "1 pop-up scheduled ({n} together); delivery is not yet confirmed",
    "not_sent": "pop-up not sent; check the Notifications view",
    "quiet": "no pop-up: quiet hours ({q}); {n} arrive together after them",
    "cap": "no pop-up: the daily cap ({cap}) is reached; {n} wait in the Notifications view",
    "batch": "no pop-up yet: {n} arrive with the next one (notify_batch_minutes)",
    "off": "no pop-up ({note}); {n} wait in the Notifications view",
    "": "nothing new to pop up",
}


def cmd_notify(a: argparse.Namespace) -> int:
    """Notifications about important new events (issue #74): opt-in, quiet hours, a daily cap, batching, and actions."""
    from . import notify

    if a.sub == "check":
        r = notify.run(wait=a.wait)
        if not r["enabled"]:
            say("notifications are off: turn them on with careerbrew config set notify_events true (careerbrew notify status shows the rules)")
            return 0
        st = notify.status()
        say(f"checked: {r['new']} new, {r['open']} open; " + NOTIFY_REASONS[r["reason"]].format(n=r["batched"], q=st["quiet_hours"], cap=st["daily_cap"], note=st["backend_note"].split(";")[0]))
        return 0
    if a.sub == "act":
        r = notify.act(a.id, a.action, via="cli")
        say(
            f"{a.id}: {r['status']}" + (f" until {r['snooze_until'][11:16]}" if r["status"] == "snoozed" else "") + (" (the suggestion is decided in the inbox too)" if r["kind"] == "suggestion" and a.action in ("accept", "dismiss") else "")
        )
        return 0
    if a.sub == "open":
        if a.id:
            notify.act(a.id, "read", via="cli")
        view = f"notifications/{a.id}" if a.id else "notifications"
        if not notify.open_view(view):
            say(f"could not open a browser here; run `careerbrew notify open{' ' + a.id if a.id else ''}` on the machine with your desktop")
            return 1
        say(f"opened the web app in your browser: #/{view}")
        return 0
    st = notify.status()
    if a.sub == "status":
        say(f"notifications: {'on' if st['enabled'] else 'off (opt in: careerbrew config set notify_events true)'}")
        say(f"pop-ups: {st['backend_note']}")
        say(
            f"rules: priority >= {st['min_priority']} (5 a page on you, 4 a DM or review request, 3 a mention, important mail, or suggestion), "
            f"quiet hours {st['quiet_hours'] or 'none'}{' (now)' if st['quiet_now'] else ''}, cap {st['daily_cap']} a day ({st['sent_today']} today), "
            f"batch {st['batch_minutes']} min, snooze {st['snooze_minutes']} min"
        )
        say(f"open: {st['open']} ({st['unread']} unread): careerbrew notify list, or the web app's Notifications view")
        return 0
    v = notify.view()
    rows = [r for r in v["items"] if a.all or r["status"] in notify.OPEN]
    say(f"notifications: {st['open']} open ({st['unread']} unread), {'on' if st['enabled'] else 'off'}")
    show = a.show and may_show_business()
    if a.show and not show:
        say("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
    for r in rows:
        say(f"- {r['id']} [{r['status']}] p{r['priority']} {r['headline']}" + (f" ({r['count']}x)" if r["count"] > 1 else "") + (f": {r['title']}" if show and r["title"] else ""))
        say(f"    why: {r['why']}. next: {r['next']}. act: careerbrew notify act {r['id']} {'|'.join(r['actions']) or 'read'}")
    if not rows:
        say("nothing open" + ("" if st["enabled"] else ": careerbrew config set notify_events true turns notifications on"))
    return 0


def cmd_day_log(a: argparse.Namespace) -> int:
    from datetime import date as _date

    from .day import log as daylog
    from .day import results

    if a.list:
        days = daylog.history()
        say(f"{len(days)} kept log(s)" + (": " + ", ".join(days[:14]) if days else ""))
        return 0
    day = _date.fromisoformat(a.date) if a.date else results.today()
    d = daylog.save(day) if not a.date or not daylog.path_for(day).exists() else None
    text = daylog.read(day) or ""
    if d:
        say(f"log for {d['day']}: {sum(len(v) for v in d['done'].values())} done, {len(d['planned'])} planned for {d['tomorrow']}, {len(d['blockers'])} blocker(s), {len(d['questions'])} question(s)")
    if a.show:
        if not may_show_business():
            say("log text hidden: stdout is not a terminal and no plugin policy allows agents to read business data (open it in the web app)")
            return 0
        say(text)
    return 0


def _say_inbox(limit: int = 10, group: str = "") -> None:
    """Groups of alike suggestions first (one line each), then each other item: what, why, from, links, and what Accept and Dismiss do."""
    from .day import inbox

    v = inbox.view()
    items, groups = v["inbox"], v["groups"]
    show = may_show_business()
    if group:
        items = [i for i in items if i.get("group") == group]
        if not items:
            say(f"no open group {group} (see: careerbrew day inbox)")
            return
    else:
        for g in groups[:limit]:
            say(f"group    {g['id']:<20} {g['label'] if show else g['kind']}  ({g['count']} alike: day accept-all or dismiss-all {g['id']} --yes --count {g['count']})")
            if show:
                say(f"         why: {g['why']}\n         from: {g['added_by']}\n         accept all: {g['accept']}\n         dismiss all: {g['dismiss']}\n         each: careerbrew day inbox --group {g['id']}")
        items = [i for i in items if not i.get("group")]
    for i in items if group else items[:limit]:
        verb = "accept or dismiss" if i["type"] == "proposal" else "answer"
        text = i["text"] if show else i["kind"]
        if not show and i["kind"] == "agent-task":
            task = (i.get("data") or {}).get("task")
            if isinstance(task, str) and re.fullmatch(r"t[0-9]+", task):
                text += f" {task}"
        say(f"{i['type']:<8} {i['id']:<20} {text}  ({verb})")
        if show and i["type"] == "proposal":
            say(f"         why: {i['why']}\n         from: {i['added_by']}")
            for x in i.get("links") or []:
                say(f"         link: {x['label']} {x['url']}" if x["label"] != x["url"] else f"         link: {x['url']}")
            if i.get("maybe_done"):
                say(f"         maybe done: {i['maybe_done']['what']}")
            say(f"         accept: {i['accept']}\n         dismiss: {i['dismiss']}")
    if not group and len(items) > limit:
        say(f"... {len(items) - limit} more of {len(items)} open (careerbrew day inbox)")
    if v["expired"] and not group:
        say(f"done elsewhere: {len(v['expired'])} suggestion(s) hidden because their source shows them done (careerbrew day reopen <id> brings one back)")


def cmd_day_send(a: argparse.Namespace) -> int:
    """Send today's PM update: Slack posts it (needs --yes); email prints the mail-draft link."""
    from urllib.parse import quote

    from .day import log as daylog

    day_data = daylog.save()
    pm = daylog.pm_report(day_data)
    text = a.text if a.text else pm["body"]
    pm["checks"] = daylog.content_checks(text, day_data)
    via = a.via or pm["via"]
    if bad := daylog.unlinked_refs(text):  # issue #227: a PR number or ticket key the reader cannot click
        events.emit("day.pm.draft", status="refused", unlinked=len(bad))
        say(f"not sent: {len(bad)} line(s) name a PR or ticket with no link; add a link (for example [#12](https://...)) or remove the number")
        if may_show_business():
            say("\n".join(f"  {x}" for x in bad))
        return 1
    if via == "email":
        say(f"mailto:{quote(str(config_get('pm_email')))}?subject={quote(pm['subject'])}&body={quote(text)}")
        say("open that link to get a draft in your mail app; you press send")
        return 0
    if not a.yes:
        if may_show_business():
            say(f"would post to {config_get('pm_slack') or '(pm_slack not set)'}:\n{pm['subject']}\n\n{text}")
            if pm["why"]:  # issue #214: owner-only notes, printed apart from the text and never sent
                say("\nWhy it matters (for you only, never sent):\n" + "\n".join(f"  {w['label']}: {w['note']}" for w in pm["why"]))
        elif pm["why"]:
            say(f"why it matters: {len(pm['why'])} note(s) for the owner (never sent; they read them in a terminal or the web app)")
        if pm["checks"]:  # issue #226: content rules a machine checks; the owner sees each line, an agent the rules only
            say(f"content checks: {len(pm['checks'])} ({', '.join(dict.fromkeys(c['rule'] for c in pm['checks']))})")
            if may_show_business():
                say("\n".join(f"  {c['fix']}: {c['line']}" for c in pm["checks"]))
        say("not sent: add --yes to post it to Slack (agents: only after the user says yes)")
        return 2
    r = daylog.send_pm(f"*{pm['subject']}*\n{text}")
    say(f"sent to Slack {r['to']}")
    return 0


def config_get(key: str) -> Any:
    from . import config

    return config.get(key)


def cmd_day_inbox(a: argparse.Namespace) -> int:
    from .day import inbox

    inbox.regenerate()
    _say_inbox(limit=100, group=getattr(a, "group", None) or "")
    if not inbox.open_items():
        say("nothing to decide or answer")
    return 0


def cmd_day_group(a: argparse.Namespace) -> int:
    """Accept all or Dismiss all of one group of alike suggestions (issue #147). Hard to undo, so it needs --yes."""
    from .day import inbox

    action = "accept" if a.sub == "accept-all" else "dismiss"
    g = next((g for g in inbox.view()["groups"] if g["id"] == a.group), None)
    if g is None:
        say(f"no open group {a.group} (see: careerbrew day inbox)")
        return 1
    if not a.yes:
        if may_show_business():
            say(f"would {action} all {g['count']}: {g['label']}\n{g['accept'] if action == 'accept' else g['dismiss']}")
        say(f"not done: add --yes --count {g['count']} to {action} all {g['count']} (agents: only after the user says yes)")
        return 2
    if a.count is not None and a.count != g["count"]:  # rows joined or left the group since the owner saw it: ask again
        say(f"not done: group {a.group} now holds {g['count']} suggestions, not {a.count}; look again (careerbrew day {a.sub} {a.group})")
        return 1
    r = inbox.decide_group(a.group, action, ids=g["ids"])
    say(f"{a.group}: {r['decided']} {action}ed" + (f", {len(r['failed'])} failed (still open)" if r["failed"] else "") + (f", {r['left']} added since (still open)" if r["left"] else ""))
    for f in r["failed"]:
        say(f"  {f['id']}: {f['error']}")
    return 1 if r["failed"] else 0


def cmd_checklist(_a: argparse.Namespace) -> int:
    """Getting started: the first steps, each done or with the command that does it (issue #147)."""
    from . import checklist

    c = checklist.build()
    say(f"getting started: {c['done']} of {c['total']} done" + (" (hidden on Dashboard: first_run_checklist is false)" if c["hidden"] else ""))
    for s in c["steps"]:
        say(f"[{'x' if s['done'] else ' '}] {s['title']}" + ("" if s["done"] else f": {s['command']} (or {s['view'].removeprefix('#/')} in the web app); {s['why']}"))
    if c["show"]:
        say("hide it on Dashboard: careerbrew config set first_run_checklist false")
    return 0


def cmd_day_suggest(a: argparse.Namespace) -> int:
    """A plugin (its skill, command, or checkup step) adds its own explained suggestion (issue #71)."""
    from .day import inbox

    i = inbox.suggest(a.plugin, a.what, a.why, a.accept, ref=a.ref or "", task=a.task or "", link=a.link or "")
    if i["status"] == "decided":
        say(f"{i['id']} was already decided; it does not come back (see: careerbrew day suggestions --plugin {a.plugin})")
    else:
        say(f"suggested {i['id']} ({i['status']}, plugin {a.plugin}): it waits in the inbox until the owner accepts or dismisses it")
    return 0


def cmd_day_suggestions(a: argparse.Namespace) -> int:
    from .day import inbox

    rows = inbox.plugin_suggestions(a.plugin or "")
    for r in rows:
        say(f"{r['id']:<20} {r['plugin']:<14} {r['status']:<10} {r['ref']}")
    say(f"{len(rows)} plugin suggestion(s)" if rows else "no plugin suggestions")
    for plugin, w in sorted({(r["plugin"], r["weight"]) for r in rows}):  # issue #185: what your decisions taught careerbrew
        say(f"{plugin}: next suggestions rank with weight {w} (1.0 = no dismissals; lower = you dismiss more than you accept)")
    return 0


def cmd_signals(a: argparse.Namespace) -> int:
    """What each of your decisions changed (issue #185): ranks, the question budget, what plugins learn."""
    from . import signals

    if a.action == "reset":
        if not (a.kind and a.source):
            say("reset needs a kind and a source, as `careerbrew signals` names them: careerbrew signals reset plugin plugin:acme")
            return 2
        n = signals.reset(a.kind, a.source)
        say(f"{a.kind} {a.source}: {n} decision(s) forgotten; " + ("the PM update writes that line as generated again" if a.kind in signals.PM_KINDS else "its weight is 1.0 again and its questions come back"))
        return 0
    rows = signals.summary()
    if not may_show_business():  # project, label, and sender names are business text: counts and kinds only (issue #185)
        rows = [{k: v for k, v in r.items() if k not in ("source", "effect")} | {"source": "(hidden)", "effect": "(hidden: run it in your terminal)"} for r in rows]
    if a.json:
        say(json.dumps(rows, indent=2))
        return 0
    if not rows:
        say("no decisions yet: accept, dismiss, or answer items in the inbox (careerbrew day inbox, or Today in the web app)")
        return 0
    say(f"{len(rows)} signal(s) from your decisions; {sum(1 for r in rows if r['signal'] == 'pm' or float(r['weight']) < 1 or r['muted'])} change what careerbrew does now")
    for r in rows:
        if r["signal"] == "pm":
            say(f"- PM update  {r['effect']}")
        else:
            say(f"- {r['kind']:<12} from {r['source']}: {r['accepted']} accepted, {r['dismissed']} dismissed -> {r['effect']}")
    return 0


def cmd_day_decide(a: argparse.Namespace) -> int:
    from .day import inbox

    if a.sub == "accept":
        i = inbox.accept(a.id)
    elif a.sub == "dismiss":
        i = inbox.dismiss(a.id)
    elif a.sub == "reopen":
        i = inbox.reopen(a.id)
    else:
        i = inbox.answer(a.id, a.text)
    say(f"{i['id']} {i['status']}")
    return 0


def cmd_actions(a: argparse.Namespace) -> int:
    """Every action careerbrew has: for agents (--json) and people. The web app runs the same list."""
    from . import actions

    rows = actions.catalog()
    if a.name:
        rows = [r for r in rows if r["name"] == a.name or r["name"].startswith(a.name + ".")]
    if a.json:
        print(json.dumps(rows, indent=1))
        return 0
    for r in rows:
        note = f"  [terminal only: {r['terminal_only']}]" if r["terminal_only"] else (f"  [confirm: {r['confirm']}]" if r["confirm"] else "")
        note += f"  [{r['agent_note']}]" if r.get("agent_note") else ""
        args = " ".join((f"--{x['name'].replace('_', '-')}" if x["flag"] else f"<{x['name']}>") for x in r["args"] if x["required"] or not x["flag"])
        say(f"{r['command']} {args}".rstrip() + f"  - {r['help']}{note}")
    say(f"{len(rows)} action(s); the web app runs them in Actions; details: careerbrew actions --json")
    return 0


def cmd_metrics_weekly(a: argparse.Namespace) -> int:
    """Opt-in weekly aggregate metrics (numbers only) for agentbrew's metrics share."""
    from . import metrics_share

    payload = metrics_share.collect()
    ok, why = metrics_share.allowed()
    if a.dry_run or not ok:
        print(json.dumps(payload, indent=1, sort_keys=True))
        say(f"metrics sharing is {why}; nothing written" if not ok else "dry run: nothing written")
        return 0
    say(f"wrote {metrics_share.write()}")
    return 0


# What each run's `minutes_spent` measured (time_saved._time).
TIME_SOURCE = {"web": "your time to copy, send, or accept in the web app", "cli": "CLI run time, an upper bound of time saved", "web+cli": "web app time, and CLI run time for CLI runs"}


def cmd_metrics_time_saved(a: argparse.Namespace) -> int:
    """Minutes saved per week: (manual baseline - time careerbrew took) per action run. Numbers only."""
    from . import time_saved

    data = time_saved.compute()
    path = time_saved.write(data)
    if a.json:
        print(json.dumps(data, indent=1, sort_keys=True))
        return 0
    for week in data["weeks"]:
        say(f"{week['week']}  {week['minutes_saved']:>7.1f} min saved  {week['runs']} run(s)")
        for action, c in week["actions"].items():
            if c["runs"]:
                say(f"  {action:<10} {c['runs']} run(s)  {c['minutes_spent']:.1f} min spent ({TIME_SOURCE[c['time']]})")
    for action, b in data["baselines"].items():
        say(f"baseline  {action:<10} {b['minutes']:>5.0f} min  ({b['kind']})")
    say(f"time-saved: {data['minutes_saved_total']} min over {data['runs_total']} run(s); {data['measured_baselines']}/{len(data['baselines'])} baselines measured; wrote {path}")
    return 0


def cmd_metrics_field(a: argparse.Namespace) -> int:
    """This machine's real-use totals from its own event log (local only; nothing is sent)."""
    from . import field

    data = field.compute()
    path = field.write(data)
    if a.json:
        print(json.dumps(data, indent=1, sort_keys=True))
        return 0
    rate = "no runs yet" if data["success_rate"] is None else f"{data['success_rate']}% ok"
    say(f"field {data['machine']}: {data['runs']} run(s) since {data['since'] or 'now'}, {rate}, {data['minutes_saved']} min saved; wrote {path}")
    return 0


def cmd_time(a: argparse.Namespace) -> int:
    """Record the manual minutes one action takes you, timed by hand once."""
    from . import time_saved

    value = time_saved.set_baseline(a.action, a.minutes)
    say(f"baseline {a.action}: {value} min (measured)" if value else f"baseline {a.action}: back to the labeled estimate")
    return 0


def cmd_telemetry(a: argparse.Namespace) -> int:
    """Anonymous usage counts to PostHog EU (US-89): status, on, off, send [--dry-run]."""
    from . import config, telemetry

    if a.sub == "on":
        config.set_value("telemetry", "true")
    elif a.sub == "off":
        config.set_value("telemetry", "false")
        say("telemetry off: nothing is sent")
        return 0
    if a.sub == "send":
        if a.dry_run:
            print(json.dumps(telemetry.dry_run(), indent=1, sort_keys=True))
            say("dry run: nothing sent")
            return 0
        res = telemetry.send()
        if res["status"] == "fail":
            say(f"send failed ({res['why']}); the events stay queued for the next send")
            return 1
        say(f"sent {res['sent']} event(s)" if res["status"] == "ok" else f"nothing sent: telemetry is {res['why']}")
        return 0
    st = telemetry.status()
    say(f"telemetry: {'on' if st['on'] else 'off'} ({st['why']})")
    say(f"endpoint: {st['endpoint']}  install id: {st['install_id']} (random)")
    say(f"pending: {st['pending']} event(s); last send: {st['last_send_day'] or 'never'}")
    say("see exactly what would be sent: careerbrew telemetry send --dry-run")
    return 0


def cmd_config(a: argparse.Namespace) -> int:
    from . import config

    if a.action == "set":
        value = config.set_value(a.key, a.value)
        say(f"{a.key} = {value} (saved to settings.yaml)")
    elif a.action == "unset":
        config.unset(a.key)
        say(f"{a.key} back to its default")
    else:
        for r in config.effective():
            say(f"{r['key']:<18} {r['value']!s:<24} {r['source']:<14} {r['help']}")
    return 0


def cmd_start(a: argparse.Namespace) -> int:
    """`careerbrew` with no arguments: start everything with sane defaults, then show what matters now."""
    from . import config, report, runs, server
    from .day import inbox, watch
    from .day import log as daylog
    from .work import projects

    step = run_say()
    steps: list[report.Item] = []
    from .day import handoff

    h_day, h_items = handoff.latest_before()
    if h_items:  # first, before anything else: what the last session left for this one (issue #210)
        step(f"handoff from {h_day}: {len(h_items)} item(s); verify each one live before acting (careerbrew day handoff show)")
        steps.append(report.Item(f"Handoff from {h_day}: {len(h_items)} item(s) to verify live", id="handoff", command="careerbrew day handoff show"))
    if not (paths.data_root() / "profile.yaml").exists():
        if step is say:
            cmd_init(a)
        else:  # the page says it; the init text would be the wall of text the page replaces
            with contextlib.redirect_stdout(io.StringIO()):
                cmd_init(a)
        steps.append(report.Item("Created the data folder from the examples", id="init", command="careerbrew init"))
    if config.get("start_watch") and not a.no_watch:
        st = watch.start()
        step(f"day watcher: {st.get('state', 'on')} (it updates your log on every new change; `careerbrew day watch off` stops it)")
        steps.append(report.Item("Day watcher on", id="watch", evidence=f"state {st.get('state', 'on')}", command="careerbrew day watch status"))
    if config.get("start_web") and not a.no_web:
        url = server.start_background(int(config.get("serve_port")))
        if sys.stdout.isatty():  # the sign-in link with its token: only in your own terminal, never on the page
            say(f"web app: {url}")
        else:
            step(f"web app: http://127.0.0.1:{config.get('serve_port')}/ (the link with its token prints in your terminal)")
        steps.append(report.Item("Web app running", id="web", evidence=f"http://127.0.0.1:{config.get('serve_port')}/ (local only, token-guarded)", command="careerbrew open"))
    if not a.no_refresh:
        stats = watch.run_cycle()
        step(f"refreshed: {stats['added']} new item(s) from your sources")
        steps.append(report.Item("Refreshed your sources", id="refresh", evidence=f"{stats['added']} new item(s)", command="careerbrew day watch once"))
    d = daylog.save()
    step(f"today: {sum(len(v) for v in d['done'].values())} done, {len(d['planned'])} planned for {d['tomorrow']}, {len(d['blockers'])} blocker(s)")
    counts = {"proposal": 0, "question": 0}
    items = inbox.open_items()
    for i in items:
        counts[i["type"]] += 1
    step(f"for you: {counts['proposal']} proposal(s), {counts['question']} question(s)")
    from .work import promises

    if owed := promises.open_list(business=False):  # promises you made (issue #268): the work-day cycle works each one
        late = sum(r["overdue"] for r in owed)
        step(f"promises you made: {len(owed)} open, {late} overdue (careerbrew work promise list)")
        steps.append(report.Item("Promises you made", id="promises", evidence=f"{len(owed)} open, {late} overdue", command="careerbrew work promise list --json"))
    from . import checklist

    first = checklist.build()
    if first["show"]:
        step(f"getting started: {first['done']} of {first['total']} first steps done (careerbrew checklist)")
    if step is say:
        _say_inbox(limit=5)
        _say_stale_sources()
    from . import telemetry

    notice = telemetry.notice_once()
    if notice:
        say(notice)
    step("change a default: careerbrew config set <key> <value> (see: careerbrew config)")
    report.finish(runs.start(steps, d, items, projects.stale_sources()), say)
    return 0


def _handoff_lines(items: list[dict], show_text: bool) -> list[str]:
    from .day import handoff

    lines = []
    for sec in handoff.SECTIONS:
        mine = [i for i in items if i["section"] == sec]
        if not mine:
            continue
        lines.append(f"{handoff.TITLES[sec]} ({len(mine)}):")
        for i in mine:
            body = i["text"] if show_text else "(text hidden)"
            lines.append(f"- [{i['id']}] {body}" + (f" {i['link']}" if i.get("link") else ""))
    return lines


def _handoff_fail(msg: str) -> int:
    say(f"error: {msg} (breaks US-85)")
    return 2


def cmd_day_handoff(a: argparse.Namespace) -> int:
    from .day import handoff

    if a.action == "add":
        if not a.section or not a.text:
            return _handoff_fail("usage: careerbrew day handoff add <section> <text> [--link URL]")
        try:
            item = handoff.add(a.section, a.text, a.link)
        except ValueError as e:
            return _handoff_fail(str(e))
        say(f"handoff: added {item['id']} to {a.section} for tomorrow")
        return 0
    if a.action == "done":
        if not a.section:
            return _handoff_fail("usage: careerbrew day handoff done <id>")
        if a.section.startswith("promise-") and not handoff.done(a.section):  # a live promise item: done closes the promise (issue #268)
            from .work import projects, promises

            try:
                promises.close(a.section, "done")
            except projects.ProjectError as e:
                return _handoff_fail(str(e))
            say(f"handoff: promise {a.section} done")
            return 0
        if not handoff.done(a.section):
            return _handoff_fail(f"no handoff item {a.section} today (see: careerbrew day handoff show --today)")
        say(f"handoff: dropped {a.section}")
        return 0
    day, items = (handoff.today(), handoff.load()) if a.today else handoff.latest_before()
    items = handoff.with_promises(items)  # open promises show until done (issue #268)
    day = day or (handoff.today() if items else None)
    show_text = may_show_business()
    from .workday import questions as workday_questions

    open_q = len(workday_questions.open_records())  # #261: open question records carry over to the next morning
    if a.json:
        print(json.dumps({"day": day.isoformat() if day else None, "verify_live_before_acting": True, "items": [{k: v for k, v in i.items() if k != "text" or show_text} for i in items], "open_questions": open_q}))
        return 0
    if open_q:
        say(f"questions: {open_q} open from the work day; recheck them before you ask: careerbrew day question recheck")
    if not items:
        say("handoff: none" + ("" if a.today else " from an earlier day"))
        return 0
    say(f"handoff from {day}: {len(items)} item(s). Verify each one live before acting.")
    for line in _handoff_lines(items, show_text):
        say(line)
    return 0


def cmd_work_promise(a: argparse.Namespace) -> int:
    """Promises you made, tracked as projects (issue #268, US-99)."""
    from .day import pm_promises
    from .work import projects, promises

    try:
        if a.action == "outcomes":
            rows = pm_promises.outcomes()
            if a.json:
                print(json.dumps({"outcomes": rows}))
            else:
                say(f"PM promise outcomes: {len(rows)}")
                for row in rows:
                    say(f"  {row['promise']} {row['state']}" + (f" {row['id']}" if row["id"] else ""))
                    if row["state"] == "decision-needs-review":
                        say(f"    prior: {row['prior_state']}" + (f" due {row['prior_due']}" if row["prior_due"] else "") + f"; current: {row['current_state']}" + (f" due {row['current_due']}" if row["current_due"] else ""))
                    for option in row.get("resolutions", []):
                        separate = f"; {'keep' if option['separate_existing'] else 'create'} a separate promise due {option['separate_due']}" if option["separate_due"] else ""
                        say(f"    {option['choice']}" + (f" due {option['due']}" if option["due"] else "") + separate + f": {option['id']}")
            return 0
        if a.action == "accept":
            if not a.yes or not (_owner_ui() or (sys.stdin.isatty() and sys.stdout.isatty())):
                say("PM promise acceptance needs owner confirmation")
                return 2
            pm_promises.accept(a.proposal_id)
            say("PM promise outcome accepted")
            return 0
        if a.action == "stats":
            counts = promises.stats(a.days)
            if a.json:
                print(json.dumps(counts))
            else:
                for key, value in counts.items():
                    say(f"{key}: {value if value is not None else 'unknown'}")
            return 0
        if a.action == "scan":
            r = promises.scan()
            say(f"promises: {r['new']} new from {r['events']} sent message(s) still kept")
            return 0
        if a.action == "add":
            name = promises.add(a.text, to=a.to or "", due=a.due or "", link=a.link or "", parent=a.parent or "")
            say(f"promise {name} tracked; close it with: careerbrew work promise done {name}")
            return 0
        if a.action in promises.CLOSED:
            promises.close(a.name, a.action)
            say(f"promise {a.name} {a.action}: it left Commitments and the handoff")
            return 0
        if a.action == "step":
            promises.step(a.name, a.step, a.text)
            p = projects.records()[a.name]
            say(f"promise {a.name}: {a.step} recorded; next: {promises.next_step(p)}")
            return 0
    except projects.ProjectError as e:
        say(f"error: {e} (breaks US-99)")
        return 2
    show = may_show_business()
    rows = promises.open_list(business=show, include_closed=a.all)
    if a.json:
        print(json.dumps({"promises": rows, "steps": promises.STEP_TEXT}))
        return 0
    if not rows:
        say("promises: none open")
        return 0
    say(f"promises: {sum(not r['closed'] for r in rows)} open" + (f", {sum(r['overdue'] for r in rows)} overdue" if any(r["overdue"] for r in rows) else ""))
    for r in rows:
        what = r.get("title") or "(text hidden)"
        to = f" to {r['to']}" if r.get("to") else ""
        when = f" due {r['due']}" + (" OVERDUE" if r["overdue"] else "") if r["due"] else ""
        state = f" [{r['closed']}]" if r["closed"] else f" next: {r['next']}"
        say(f"- {r['name']}{when}: {what}{to}{state}" + (f" {r['url']}" if r["url"] else ""))
    return 0


def cmd_day_results(a: argparse.Namespace) -> int:
    from datetime import date as _date
    from datetime import datetime

    from . import calendar
    from .day import results

    day = _date.fromisoformat(a.date) if a.date else results.today()
    rows = results.load(day)
    say(f"results of {day}: {len(rows)} item(s): " + (", ".join(f"{k}={v}" for k, v in results.summary(day).items()) or "none"))
    if a.show:
        if not may_show_business():
            say("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data")
            return 0
        tz = calendar.local_tz()

        def local_time(row: dict[str, Any]) -> datetime | None:
            try:
                at = datetime.fromisoformat(str(row.get("at", "")).replace("Z", "+00:00"))
                return (at if at.tzinfo else at.replace(tzinfo=tz)).astimezone(tz)
            except ValueError:
                return None

        timed = [(local_time(r), r) for r in rows]
        for at, r in sorted(timed, key=lambda item: item[0].timestamp() if item[0] is not None else float("-inf")):
            when = at.strftime("%H:%M") if at else ""
            say(f"- {when} [{r.get('kind')}] {r.get('title', '')}")
    return 0


def cmd_day_plan(a: argparse.Namespace) -> int:
    from datetime import date as _date

    from . import report, runs
    from .day import planner

    step = run_say()
    plan = planner.build(_date.fromisoformat(a.date) if a.date else None)
    step(f"plan for {plan['day']}: {len(plan['meetings'])} meeting(s), {plan['free_minutes']} free min, {plan['focus_minutes']} focus min, estimates x{plan['calibration']}")
    show = may_show_business()
    for item in plan["schedule"]:
        title = item["title"] if show else "(title hidden; run in your terminal)"
        carry = " [carry-over]" if item["carry_over"] else ""
        step(f"  {item['start'][11:16]}-{item['end'][11:16]}  {item['task']}  {title}{carry}  ({item['reason']})")
    if plan["unplaced"]:
        step(f"  not today (no room): {', '.join(plan['unplaced'])}")
    for err in plan["calendar_errors"]:
        step(f"  calendar error {err}")
    report.finish(runs.day_plan(plan), say)
    return 0


def _workday_fail(msg: str, story: str = "US-94") -> int:
    say(f"error: {msg} (breaks {story})")
    return 2


def _workday_row(row: dict[str, Any], show_text: bool) -> dict[str, Any]:
    """A status row for an agent: step, status, time, and whether it needs the user; its text only when business data may show."""
    out = {"step": row["step"], "status": row["status"], "at": row.get("at", ""), "needs_you": bool(row.get("needs_you"))}
    # #259: content-free truth fields (the next action is generic text with the step name only)
    out |= {k: row.get(k) for k in ("started", "attempt", "overdue", "verify", "error_class", "next")}
    out |= {k: row.get(k) for k in ("ended", "seconds", "median", "slow")}  # #263: timing, content-free
    if show_text:
        out["text"] = {k: row.get(k, "") for k in ("summary", "needs_you", "link")}
    return out


def _public_workday_about(step: dict[str, Any]) -> str:
    path = step.get("path")
    if step.get("from") != "careerbrew" or not isinstance(path, str) or not path:
        return ""
    try:
        from .agent_workflows import repository_workflows

        target = Path(path).expanduser().resolve()
        if target.is_relative_to(paths.data_root(create=False).resolve()) or target.is_relative_to((paths.REPO_ROOT / "private").resolve()):
            return ""
        inventory = repository_workflows(paths.REPO_ROOT)
        return step["about"] if not inventory.problems and any(entry.path.resolve() == target for entry in inventory.entries) else ""
    except (OSError, RuntimeError, ValueError):
        return ""


def cmd_day_run(a: argparse.Namespace) -> int:
    """The work day plan for `/work` (issue #257): steps from careerbrew, the work_steps setting, and plugin manifests."""
    from .workday import plan as workday_plan
    from .workday import start as workday_start
    from .workday import steps as workday_steps

    lookup: workday_start.CheckoutLookup = {}
    try:
        p = workday_plan.build(lookup=lookup)
    except workday_steps.StepError as e:
        return _workday_fail(str(e))
    show = may_show_business()
    if a.json:
        dash = p["dashboard"]
        # a verify or cycle command, or a watched repo path, may name company hosts: an agent sees only that one is set
        shown = p["steps"] if show else [{**s, "about": _public_workday_about(s), "verify": bool(s["verify"]), "cycle": bool(s["cycle"]), "watch": [i.split(":", 1)[0] for i in s["watch"]]} for s in p["steps"]]
        out = {**p, "steps": shown, "dashboard": {**dash, "rows": [_workday_row(r, show) for r in dash["rows"]]}}
        if not show:
            from .day import pm_promises
            from .workday import questions as workday_questions

            out["pm_promises"] = pm_promises.public(out["pm_promises"])
            out["question_records"] = [workday_questions.public(q) for q in p["question_records"]]  # a question's text is business data (#261)
            out = workday_start.redact(out, workday_start.labels(p["steps"], lookup=lookup))  # a plugin's folder name may name the company (#260)
        print(json.dumps(out))
        return 0
    c = p["dashboard"]["counts"]
    say(f"{p['dashboard']['title']} ({p['machine']} machine): {len(p['steps'])} step(s); {c['done']} done, {c['running']} running, {c['waiting']} waiting, {c['blocked']} blocked, {c['failed']} failed, {c['needs_you']} need(s) you")
    say(p["rule"])
    for name in p.get("about_missing", []):
        say(f"warn: {name} has no `about` and its file has no description; add `about:` to its work step (docs/plugins.md)")
    for n, (s, row) in enumerate(zip(p["steps"], p["dashboard"]["rows"], strict=True), 1):
        origin = "" if s["from"] == "careerbrew" else f" (from {s['from']})"
        missing = "" if s["path"] else "  [not installed here]"
        text = f"  {row['summary']}" if show and row.get("summary") else ""
        flag = "  [overdue: run careerbrew day check]" if row.get("overdue") else ""
        src = f"  [{s['source']['commit'][:8]}]" if s.get("source", {}).get("commit") else ""
        say(f"  {n}. {s['step']:<24} {row['status']:<8} stage {s['stage']}{origin}{missing}{flag}{src}{text}")
    say(f"stages: {len(p['stages'])}; why each step waits: careerbrew day run --json (steps[].why)")
    if p["question_records"]:
        say(f"questions: {len(p['question_records'])} open; ask them once after the last step: careerbrew day question recheck")
    if p["handled"]:
        say(f"handled today: {len(p['handled'])} checkout(s) or PR(s); later steps skip them")
    say("agents: careerbrew day run --json; update a row: careerbrew day update <step> --status <status>; the page: careerbrew day render")
    return 0


def cmd_day_update(a: argparse.Namespace) -> int:
    from .workday import gate
    from .workday import plan as workday_plan
    from .workday import status as workday_status
    from .workday.steps import normalize

    if a.status is None and a.summary is None and a.needs_you is None and a.link is None and not a.handled and a.error_class is None:
        return _workday_fail("nothing to change: pass --status, --summary, --needs-you, --error-class, --link, or --handled")
    try:
        declared = workday_plan.build()["steps"]  # a plan that cannot be staged takes no update
        names = [s["step"] for s in declared]
        this = next((s for s in declared if s["step"] == normalize(a.step)), {})
        command = this.get("verify", "")
        st, cls, verified, why = a.status, a.error_class, None, ""
        if cls is not None and st not in ("failed", None):
            raise ValueError("--error-class goes only with --status failed: a failed row says what broke (blocked rows wait on you instead)")
        if st == "done" and command and not gate.may_run(this["from"]):
            raise ValueError(f"the verify of {this['step']} comes from {this['from']}, which may not run commands: careerbrew plugin add <src> --allow-exec")
        if st == "done" and command and normalize(a.step) in names:
            ok, why = gate.run_verify(command)  # done only on proof (#259); its output is never read
            st, cls, verified = ("done", cls, "pass") if ok else ("failed", "tool", f"fail: {why}")
            events.emit("workday.verify", ok=ok)
        row = workday_status.update(a.step, names, st, a.summary, a.needs_you, a.link, handled_items=a.handled, error_class=cls, verify=verified)
        if a.needs_you is not None or st is not None:
            from .workday import questions as workday_questions

            workday_questions.from_row(row, reopen=st == "blocked")  # #261: a row's ask is a question record too
    except ValueError as e:
        story = "US-100" if any(w in str(e) for w in ("blocked needs", "failed needs", "error class", "--error-class", "--allow-exec")) else "US-94"
        return _workday_fail(str(e), story)
    p = workday_plan.build()
    c = p["dashboard"]["counts"]
    ask = "; needs you" if row["needs_you"] else ""
    cls_note = f" ({row['error_class']})" if row["status"] == "failed" else ""
    say(f"work day: {row['step']} is {row['status']}{cls_note}{ask} ({c['done']}/{len(p['steps'])} done); the page: careerbrew day render")
    if verified and verified != "pass":
        say(f"  verify {why}: the step is not done until its verify passes; next: {row['next']}")
        return 1
    if row["next"]:
        say(f"  next: {row['next']}")
    return 0


def _error_classes() -> tuple[str, ...]:
    from .workday.gate import ERROR_CLASSES

    return ERROR_CLASSES


def cmd_day_check(a: argparse.Namespace) -> int:
    """The next pass (#259): a running step past its timeout is set back for one retry, then marked failed."""
    from .workday import gate
    from .workday import plan as workday_plan
    from .workday.steps import StepError

    try:
        plan = workday_plan.build()
        declared = plan["steps"]
        got = gate.check(declared)
        from .workday import stages as workday_stages
        from .workday import status as workday_status

        got["ready"] = workday_stages.ready(declared, workday_status.rows([s["step"] for s in declared]))
    except StepError as e:
        return _workday_fail(str(e), "US-100")
    wrap_up = plan.get("wrap_up", {})
    held = wrap_up.get("phase") == "held"
    if a.json:
        if held:
            got["wrap_up"] = wrap_up
        print(json.dumps(got))
        return 0
    limits = {s["step"]: s["timeout"] for s in declared}
    for step in got["retry"]:
        say(f"  {step} timed out after {limits[step]} min: retry {gate.MAX_ATTEMPTS} of {gate.MAX_ATTEMPTS} (run {step} again)")
    for step in got["failed"]:
        say(f"  {step} timed out again after {limits[step]} min: failed (tool); careerbrew day run shows its next action")
    if held:
        say("automatic day end is paused. Resume with careerbrew day wrap --at HH:MM.")
    say("ready: " + (", ".join(got["ready"]) or "none"))
    say(f"work day check: {got['checked']} running, {len(got['retry'])} to retry, {len(got['failed'])} failed after a timeout")
    return 0


def cmd_day_start(a: argparse.Namespace) -> int:
    """The start of the work day (#260): fast-forward the checkouts, check the shell, probe sign-in, once, before stage 1."""
    from .workday import plan as workday_plan
    from .workday import start as workday_start
    from .workday.steps import StepError

    if getattr(a, "fix", False) and getattr(a, "update_only", False):
        return _workday_fail("run source updates and connector repair separately", "US-102")
    if getattr(a, "update_only", False):
        from .workday import self_update

        try:
            got = self_update.run()
        except StepError as e:
            return _workday_fail(str(e), "US-102")
        if not may_show_business():
            got = workday_start.redact(got, workday_start.labels([]))
        if a.json:
            print(json.dumps(got))
        else:
            say(f"updated: {'yes' if got['updated'] else 'no'}")
            for name in got["changed_files"]:
                say(f"  changed: {name}")
            for result in got["refresh"]:
                say(f"  {result['path']}: {result['status']}")
            for result in got["repairs"]:
                say(f"  {result['check']}: {'ok' if result['ok'] else 'failed; repair this before starting /work'}")
            say("Re-read commands/work.md and the careerbrew-work-day skill after updated: yes; then read the new day plan.")
        return 0 if got["ok"] else 1

    if getattr(a, "fix", False):
        from .workday import mcp_check

        try:
            result = mcp_check.fix_github_headers(yes=bool(a.yes))
        except ValueError as exc:
            return _workday_fail(str(exc), "US-102")
        if a.json:
            print(json.dumps(result))
        else:
            say(f"GitHub connector repair: {result['changed']} configured; {result['verification']}.")
            say(result["next"])
        return 0

    try:
        declared = workday_plan.build()["steps"]
        got = workday_start.run(declared)
    except StepError as e:
        return _workday_fail(str(e), "US-102")
    if not may_show_business():
        got = workday_start.redact(got, workday_start.labels(declared))  # a plugin's folder name may name the company
    if a.json:
        print(json.dumps(got))
        return 0
    moved = [r for r in got["refresh"] if r["status"] == "fast-forwarded"]
    left = [r for r in got["refresh"] if r["status"] in workday_start.NOT_TOUCHED]
    for r in got["refresh"]:
        if r["status"] == "fast-forwarded":
            note = f"fast-forwarded {r['behind']} commit(s) {r['before']} -> {r['after']}"
        elif r["status"] in workday_start.NOT_TOUCHED:
            note = f"not touched: {r['status']}"
        else:
            note = f"{r['status']} at {r['before']}"
        dup = f" (same remote as {r['duplicate_of']}: not watched)" if r["duplicate_of"] else ""
        say(f"  {r['path']}  {note}{dup}")
    if any(r["for"] and r["for"][0].startswith("plugin:") for r in moved):
        say("  a plugin's folder moved: careerbrew plugin refresh installs the new version")
    traps = [c for c in got["preflight"] if workday_start.needs_attention(c)]
    for c in traps:
        label = "warn" if c.get("status") == "warn" else "trap"
        say(f"  {label}  {c['check']} {c['value']}: {c['safe']}")
    say(f"  every step reads the shell traps first: {workday_start.shown(workday_start.TRAPS)}")
    sign = got["signin"]
    if sign["status"] == "failed":
        say(f"  sign-in probe failed ({sign['why']}): {', '.join(sign['steps'])} need sign-in; operator-turn browser steps stay blocked until sign-in succeeds; other browser steps run after independent work")
        for q in got["questions"]:
            say(f"  needs you: {q['text']}")
    elif sign["status"] == "no probe":
        say(f"  sign-in: no probe set for {', '.join(sign['steps'])} (careerbrew config set work_signin_probe '<command>')")
    h = got.get("handoff") or {}
    if h.get("from"):  # issue #265: each item of the last handoff with its recheck result
        n = {st: sum(1 for i in h["items"] if i["state"] == st) for st in ("open", "closed", "agent", "unknown")}
        say(f"  handoff from {h['from']}: {len(h['items'])} item(s) rechecked: {n['open']} still open, {n['closed']} closed (dropped), {n['agent']} for an agent to check, {n['unknown']} unknown")
        for i in h["items"]:
            if i["state"] != "closed":
                say(f"    [{i['id']}] {i['section']} {i['state']}: {i['why']}")
        say("  the open items: careerbrew day handoff show")
    say(f"work day start: {len(got['refresh'])} checkout(s), {len(moved)} fast-forwarded, {len(left)} not touched; {len(traps)} shell trap(s); sign-in {sign['status']}; {len(got['questions'])} question(s)")
    return 0


def _question_line(q: dict[str, Any], show: bool) -> str:
    state = f" [{q['state']}]" if q.get("state") and q["state"] != "open" else ""
    head = f"  {q['id']}  {q['step']}  {q['age']} old{state}" + (f" ({q['why']})" if q.get("state") == "unknown" else "")
    if not show:
        return head + "  (text hidden)"
    opts = ", ".join(f"{o} (pick)" if o == q.get("pick") else o for o in q.get("options", []))
    pick = f"; pick: {q['pick']}" if q.get("pick") and not opts else ""
    return head + f"  {q['text']}" + (f"  [{opts}]" if opts else "") + pick


def cmd_day_question(a: argparse.Namespace) -> int:
    """Question records (#261): a step saves each ask and never asks mid-run; recheck runs before the one batched ask."""
    from .workday import plan as workday_plan
    from .workday import questions as workday_questions
    from .workday.steps import StepError, normalize

    show = may_show_business()
    try:
        if a.action == "add":
            declared = workday_plan.build()["steps"]
            step = normalize(a.step)
            this = next((s for s in declared if s["step"] == step), None)
            if this is None:
                raise ValueError(f"step {step} is not in today's plan (steps: {', '.join(s['step'] for s in declared)}; see `careerbrew day run`)")
            if a.plugin and a.plugin not in {p["name"] for p in plugins.installed()}:
                raise ValueError(f"plugin {a.plugin} is not installed (see `careerbrew plugin list`)")
            origin = this.get("from", "")
            if a.plugin and origin.startswith("plugin:") and origin != f"plugin:{a.plugin}":
                raise ValueError(f"step {step} comes from {origin}: its question's recheck is that plugin's, so leave out --plugin")
            source = origin if origin.startswith("plugin:") else f"plugin:{a.plugin}" if a.plugin else "agent"
            q = workday_questions.add(step, a.text, a.option, a.pick or "", a.recheck or "", source)
            if q.get("closed"):
                say(f"question {q['id']} for {step} was already closed ({q['closed']}): it is not asked again")
                return 0
            say(f"question {q['id']} saved for {step} ({workday_questions.kind(q['recheck'])} recheck); ask it after the last step: careerbrew day question recheck")
            return 0
        if a.action == "close":
            if not workday_questions.close(a.id, "answered"):
                raise ValueError(f"no open question {a.id} (see: careerbrew day question list)")
            say(f"question {a.id} closed")
            return 0
    except (StepError, ValueError) as e:
        return _workday_fail(str(e), "US-104")
    if a.action == "recheck":
        got = workday_questions.recheck()
        if a.json:
            out = got if show else {**got, "ask": [workday_questions.public(q) for q in got["ask"]]}
            print(json.dumps(out))
            return 0
        for q in got["ask"]:
            say(_question_line(q, show))
        for d in got["dropped"]:
            say(f"  {d['id']}  {d['step']}  {d['age']} old  dropped: {d['why']}")
        if got["agent"]:
            say(f"  check each [agent] item's link yourself, then: careerbrew day question close <id> ({len(got['agent'])} item(s))")
        say(f"questions: {len(got['ask'])} to ask, {len(got['dropped'])} dropped as closed, {len(got['agent'])} for an agent to check")
        return 0
    rows = workday_questions.open_records()
    if a.json:
        print(json.dumps({"questions": rows if show else [workday_questions.public(q) for q in rows]}))
        return 0
    for q in rows:
        say(_question_line(q, show))
    say(f"questions: {len(rows)} open" + ("; before the batched ask: careerbrew day question recheck" if rows else ""))
    return 0


def _loop_line(lp: dict[str, Any]) -> str:
    step = f" for {lp['step']}" if lp.get("step") else ""
    return f"{lp['kind']} {lp['id']}{step} (since {lp['started'][11:16]} on {lp['day']})"


def cmd_day_loop(a: argparse.Namespace) -> int:
    """The one work-day watch loop (issue #264): a pass under a lock, an interval from measured time, recorded loops."""
    from .work import sessions
    from .workday import end as workday_end
    from .workday import watch
    from .workday.steps import StepError

    try:
        if a.action == "catch-up":
            got = watch.catch_up()
            result = got.get("pass") or {}
            failed = result.get("live", {}).get("status") == "failed" or any(row["result"] == "failed" for row in result.get("steps", []))
            if a.json:
                if result:
                    got = {**got, "pass": {k: v for k, v in result.items() if k != "live"} | {"live": {"status": result.get("live", {}).get("status", "")}}}
                print(json.dumps(got))
            else:
                say(f"turn-start catch-up: {got['status']} (late by more than 5 min)")
            return 1 if failed else 0
        if a.action == "add":
            if not a.kind or not a.id:
                raise watch.WatchError("add needs --kind and --id: the loop's kind (cron, monitor, agent, session) and its tool's id")
            lp = watch.add(a.kind, a.id, a.step or "")
            say(f"recorded {lp['kind']} {lp['id']}: end of day checks it stopped (careerbrew day loop end)")
            return 0
        if a.action == "stop":
            if not a.id:
                raise watch.WatchError("stop needs --id: the loop's id from careerbrew day loop status")
            watch.stop(a.id)
            say(f"stopped {a.id}")
            return 0
        if a.action == "end":
            got = watch.end()
            if a.json:
                print(json.dumps({"stopped": [lp["id"] for lp in got["stopped"]], "running": [{k: lp[k] for k in ("kind", "id", "step")} for lp in got["running"]]}))
            else:
                for lp in got["stopped"]:
                    say(f"  stopped {_loop_line(lp)}")
                for lp in got["running"]:
                    fix = (
                        f'careerbrew work session handoff {lp["id"]} "<text>", then careerbrew work session end {lp["id"]}'
                        if lp["kind"] == "session" and sessions.find(lp["id"])
                        else f"stop it with its own tool, then careerbrew day loop stop --id {lp['id']}"
                    )
                    say(f"  still running {_loop_line(lp)}: {fix}")
                say(f"end of day: {len(got['stopped'])} stopped, {len(got['running'])} loop(s) running")
            return 1 if got["running"] else 0
        if a.action == "run":
            n = watch.run()
            say(f"watch loop ended after {n} pass(es)")
            return 0
        if a.action == "pass":
            got = watch.run_pass()
            if a.json:
                print(json.dumps({k: v for k, v in got.items() if k != "live"} | {"live": {"status": got.get("live", {}).get("status", "")}}))
                return 1 if got["status"] == "failed" else 0
            if got["status"] == "skipped":
                say("skipped: previous cycle running (it re-arms itself; nothing to do)")
                return 0
            live_st = got["live"]["status"]
            say(f"  live feed: {live_st}" + (f" (not stored: {', '.join(got['live']['not_stored'])}; pull them again)" if got["live"].get("not_stored") else ""))
            for r in got["steps"]:
                say(f"  {r['step']} {r['result']}" + (f": {r['why']}" if r["why"] else ""))
            for lp in got["stale"]:
                say(f"  survived an earlier day: {lp['id']} ({lp['kind']}); stop it, then careerbrew day loop stop --id {lp['id']}")
            w = got["wrap_up"]  # issue #265: the clock's phase
            if w["ended"]:
                say(f"  end of day by clock ({w['end_at'][11:16]}): PM draft ready on the Dashboard ({workday_end.DASHBOARD}); handoff written; careerbrew's loops stopped (careerbrew day loop end lists the rest)")
            elif w["phase"] == "wrap-up":
                say(
                    f"  wrap-up since {w['wrap_at'][11:16]}"
                    + (f" (reminder: {w['reminded']})" if w["started"] else "")
                    + f": the loops keep running; tomorrow's handoff: careerbrew day handoff show --today; end the day: careerbrew day end (it ends by itself at {w['end_at'][11:16]})"
                )
            failed = [r for r in got["steps"] if r["result"] == "failed"] + ([got["live"]] if live_st == "failed" else [])
            say(f"watch pass: {len(got['steps'])} step cycle(s), {len(failed)} failed, {got['seconds']:g} s; next pass at {got['next_at'][11:16]} (every {got['interval']} s, {watch.FACTOR:g} x the median pass)")
            return 1 if failed else 0
        st = watch.status()
        if a.json:
            print(json.dumps({**st, "running": [{k: lp[k] for k in ("kind", "id", "step", "day")} for lp in st["running"]], "stale": [lp["id"] for lp in st["stale"]]}))
            return 0
        say(f"watch loop: {len(st['passes'])} measured pass(es), every {st['interval']} s; next pass at {st['next_at'][11:16] or 'not armed'}; re-arm: careerbrew day loop pass")
        for lp in st["running"]:
            say(f"  running {_loop_line(lp)}" + ("  [survived an earlier day]" if lp in st["stale"] else ""))
        say(f"{len(st['running'])} loop(s) running; end of day: careerbrew day loop end")
        return 0
    except (watch.WatchError, StepError, sessions.SessionError) as e:
        return _workday_fail(str(e), "US-103")


def cmd_day_end(a: argparse.Namespace) -> int:
    """End of the work day (issue #265): careerbrew's PM draft, tomorrow's handoff, the loops stopped, the page. No new report."""
    from .workday import end as workday_end
    from .workday.steps import StepError

    try:
        got = workday_end.finish()
    except StepError as e:
        return _workday_fail(str(e), "US-106")
    pm, h, loops = got["pm"], got["handoff"], got["loops"]
    if not may_show_business():  # a plugin's step name may name the company (#260)
        from .workday import plan as workday_plan
        from .workday import timing

        try:
            declared = workday_plan.build()["steps"]
        except StepError:
            declared = []
        names = timing.labels(declared, sorted({lp["step"] for lp in loops["running"] if lp.get("step")}))
        loops = {**loops, "running": [{**lp, "step": names.get(lp["step"], lp["step"])} for lp in loops["running"]]}
    if got.get("already") and not a.json:
        if not loops["running"]:
            say(f"end of day: already ended at {got['at']} (by {got['by']}); nothing changed. The PM draft is on the Dashboard: {got['dashboard']}")
            return 0
        say(f"end of day: already requested at {got['at']}; waiting for {len(loops['running'])} loop(s) or project reports before they can stop.")
    if a.json:
        print(
            json.dumps(
                {k: v for k, v in got.items() if k not in ("pm", "loops")}
                | {"pm": {k: v for k, v in pm.items() if k not in ("body", "subject")}, "loops": {"stopped": [lp["id"] for lp in loops["stopped"]], "running": [{k: lp[k] for k in ("kind", "id", "step")} for lp in loops["running"]]}}
            )
        )
        return 1 if loops["running"] else 0
    state = "your edit" if pm["edited"] else "generated"
    say(f"  PM draft ({state}, {pm['lines']} line(s), not sent): edit or send it on the Dashboard: {got['dashboard']}")
    if pm["unlinked"] or pm["checks"]:
        say(f"    fix first: {pm['unlinked']} line(s) name a PR or ticket with no link, {pm['checks']} content check(s)")
    if may_show_business():
        say(f"\n{pm['subject']}\n{pm['body']}\n")
    say(f"  handoff for tomorrow: {h['prs']} open PR(s), {h['questions']} open question(s), {h['other']} other item(s), each rechecked by tomorrow's careerbrew day start: careerbrew day handoff show --today")
    for lp in loops["stopped"]:
        say(f"  stopped {_loop_line(lp)}")
    for lp in loops["running"]:
        if lp["kind"] == "session":
            from .work import sessions

            say(f"  still running {_loop_line(lp)}: send the graceful note once: {sessions.WRAP_NOTE} Then careerbrew work session end {lp['id']} after its fresh report.")
        else:
            say(f"  still running {_loop_line(lp)}: stop it with its own tool, then careerbrew day loop stop --id {lp['id']}")
    if got["page"]:
        say(f"page: {got['page']}")
    say(f"end of day: PM draft ready (not sent), {h['prs'] + h['questions'] + h['other']} handoff item(s), {len(loops['stopped'])} loop(s) stopped, {len(loops['running'])} running")
    return 1 if loops["running"] else 0


def cmd_day_wrap(a: argparse.Namespace) -> int:
    from .workday import end

    try:
        got = end.control(a.at or "", bool(a.hold))
    except ValueError as exc:
        return _workday_fail(str(exc), "US-106")
    if a.json:
        print(json.dumps(got))
    elif got["hold"]:
        say("wrap-up: held; automatic wrap-up and clock ending are paused. Resume with careerbrew day wrap --at HH:MM, or explicitly end now with careerbrew day end.")
    else:
        say(f"wrap-up: {got['at'][11:16]}; end {got['end_at'][11:16]}. Explicit careerbrew day end ends now, including while held.")
    return 0


def _history_days(v: str) -> int:
    from .workday.timing import KEEP_DAYS

    if not v.isdigit() or not 1 <= int(v) <= KEEP_DAYS:
        raise argparse.ArgumentTypeError(f"use a whole number of days from 1 to {KEEP_DAYS} (the history keeps {KEEP_DAYS})")
    return int(v)


def cmd_day_history(a: argparse.Namespace) -> int:
    """How the work day went across days (#263): each step's runs, done, blocked, failed, and median time; each day's totals."""
    from .workday import plan as workday_plan
    from .workday import status as workday_status
    from .workday import timing
    from .workday.steps import StepError

    try:
        declared = workday_plan.build()["steps"]
    except StepError as e:
        return _workday_fail(str(e), "US-105")
    got = timing.summary(workday_status.today(), a.days)
    if not may_show_business():  # a plugin's step name may name the company
        names = timing.labels(declared, sorted({s["step"] for s in got["steps"]}))
        got["steps"] = sorted(({**s, "step": names[s["step"]]} for s in got["steps"]), key=timing.order)  # re-sorted: no hidden name's order shows
    if a.json:
        print(json.dumps(got))
        return 0
    for s in got["steps"]:
        mid = f", median {timing.shown(s['median'])}" if s["median"] is not None else ""
        say(f"  {s['step']:<24} {s['runs']} run(s): {s['done']} done, {s['blocked']} blocked, {s['failed']} failed{mid}")
    for d in got["days"]:
        say(f"  {d['day']}  {d['steps']} step(s): {d['done']} done, {d['blocked']} blocked, {d['failed']} failed, {timing.shown(d['seconds'])} of steps")
    worst = next((s for s in got["steps"] if s["failed"]), None)
    most = f"; fails most: {worst['step']} ({worst['failed']})" if worst else ""
    say(f"work day history {got['since']} to {got['until']}: {len(got['days'])} day(s), {len(got['steps'])} step(s){most}")
    return 0


def cmd_day_render(_a: argparse.Namespace) -> int:
    from . import report, runs
    from .workday import plan as workday_plan
    from .workday.steps import StepError

    try:
        r = runs.work_day(workday_plan.build())
    except StepError as e:
        return _workday_fail(str(e))
    if report.finish(r, say) is None:
        say(report.summary_line(r) + " (no page: run_pages is off; careerbrew config set run_pages true)")
    return 0


def _draft_fail(msg: str, code: int = 2) -> int:
    say(f"error: {msg} (breaks US-98)")
    return code


def _draft_text(path: str) -> str:
    try:
        return Path(path).expanduser().read_text(encoding="utf-8")
    except OSError as e:
        raise ValueError(f"could not read {Path(path).name}: {e.__class__.__name__}") from None


def cmd_draft_fit(a: argparse.Namespace) -> int:
    """What a reply draft should be, judged on the thread's newest message (issue #269). Echoes no thread or draft text."""
    from .work import drafts

    try:
        thread = drafts.load_thread(Path(a.thread).expanduser())
        r = drafts.fit(thread, _draft_text(a.draft) if a.draft else None, a.seen, a.major)
    except ValueError as e:
        return _draft_fail(str(e))
    if a.json:
        print(json.dumps(r))
        return 0
    say(f"draft: {r['kind']} ({r['reason']})")
    if r["emoji"]:
        say(f"emoji: :{r['emoji']}:")
    if r["text"]:
        say(f"text: {r['text']}")
    if r["stale"]:
        say(f"stale: {r['newer']} newer message(s) since the one your draft answers")
    if r["kind"] in ("ack", "stop", "reply") and (r["text"] or r["keep_draft"]):
        say("save it: careerbrew draft replace <target> --file <draft file>")
    return 0


def cmd_draft_replace(a: argparse.Namespace) -> int:
    """Save a reply draft safely: clear the old one, save, read back, compare, under the target's lock."""
    from .work import drafts

    try:
        r = drafts.replace(a.target, _draft_text(a.file), session=a.session or "")
    except (drafts.DraftMismatch, drafts.DraftClaimed) as e:
        return _draft_fail(str(e), 1)
    except ValueError as e:
        return _draft_fail(str(e))
    if a.json:
        print(json.dumps({k: r[k] for k in ("same", "sent_chars")} | {"uses": drafts.uses_entry(r["target"]), "storage": "local", "chat_tool_saved": False}))
        return 0
    say(f"draft saved and read back: {r['sent_chars']} characters match; stored locally, not saved in the chat tool; save it there separately when needed; a step that writes it declares `uses: {drafts.uses_entry(r['target'])}`")
    return 0


def cmd_draft_show(a: argparse.Namespace) -> int:
    """Read one saved draft (a live cycle's reply draft or review notes, issue #266). The text shows only where business data may show."""
    from .work import drafts

    try:
        text = drafts.LocalBackend().read(drafts.check_target(a.target))
    except ValueError as e:
        return _draft_fail(str(e))
    if not drafts.normalize(text):
        return _draft_fail(f"no draft saved for {a.target} (the live cycle lists each target it saved: careerbrew work live cycle)", 1)
    if may_show_business():
        print(text)
        return 0
    say(f"draft {a.target}: {len(drafts.normalize(text))} characters, never sent (text hidden: stdout is not a terminal and no plugin policy allows agents to read business data)")
    return 0


def cmd_draft_check(a: argparse.Namespace) -> int:
    """Compare a draft you saved in a chat tool with what it reads back; exit 1 when they differ. Prints no text."""
    from .work import drafts

    try:
        r = drafts.compare(_draft_text(a.sent), _draft_text(a.read_back))
    except ValueError as e:
        return _draft_fail(str(e))
    if not r["same"]:
        return _draft_fail(f"the read-back differs at line {r['line']} ({r['read_chars']} vs {r['sent_chars']} characters): the chat tool kept other text", 1)
    say(f"draft check: the read-back matches ({r['sent_chars']} characters)")
    return 0


def cmd_draft_route(a: argparse.Namespace) -> int:
    """Where an access ask goes (issue #314): the owning channel, and an open thread to link instead of a new one. Prints ids, never text."""
    from .work import access

    try:
        r = access.route(_draft_text(a.ask), access.load(Path(a.directory)), a.teammate or "")
    except ValueError as e:
        return _draft_fail(str(e))
    if a.json:
        print(json.dumps(r))
        return 0
    where = {"link": f"link the open thread {r.get('thread')} in {r.get('channel')}", "open": f"draft the ask in {r.get('channel')}"}.get(r["action"], r["action"])
    say(f"access route: {where} ({r['reason']})" + (f"; teammate: {r['tag']}" if r.get("tag") else ""))
    return 0


def cmd_draft_judge(a: argparse.Namespace) -> int:
    """The channel check before a draft is saved (issue #314): owning channel, no duplicate thread, every mention a member. Exit 1 on a failure."""
    from .work import access

    try:
        fails = access.judge(_draft_text(a.draft), a.channel, access.load(Path(a.directory)), a.new_thread)
    except ValueError as e:
        return _draft_fail(str(e))
    if fails:
        return _draft_fail("draft judge: " + "; ".join(fails) + " (fix the draft and judge it again; never send)", 1)
    say(f"draft judge: passes for {a.channel}")
    return 0


def cmd_report(a: argparse.Namespace) -> int:
    """Run pages. `latest --json` is for agents: counts, ids, statuses, and next-action commands, never page text."""
    from . import report

    if a.sub == "list":
        rows = report.all_summaries()[: a.top]
        if a.json:
            say(json.dumps(rows, indent=1))
            return 0
        for s in rows:
            say(f"{s['id']:<36} {s['status']:<8} {len(s['done'])} done, {len(s['blockers'])} blocker(s), {len(s['next'])} next")
        say(f"{len(rows)} run page(s) in {report.folder()}; open one: careerbrew open --report <id>" if rows else NO_PAGES)
        return 0
    s = report.latest(a.run)
    if not s:
        have = sorted({str(x["run"]) for x in report.all_summaries()})
        say(f"no run page for {a.run} (runs with pages: {', '.join(have)})" if a.run and have else NO_PAGES)
        return 1
    if a.json:
        say(json.dumps(s, indent=1))
        return 0
    say(f"page: {report.folder() / (s['id'] + '.html')}")
    for line in report.text_lines(s):
        say(line)
    return 0


def cmd_report_public(a: argparse.Namespace) -> int:
    """The repo health export (`repo_health.py`): the only careerbrew data an agent may publish."""
    from . import repo_health

    try:
        payload = repo_health.collect(a.days)
        written = repo_health.write_rows(payload, Path(a.out).expanduser()) if a.out else []
    except repo_health.PublicError as exc:
        events.emit("report.public", status="refused")
        say(f"public export refused: {exc}")
        return 1
    if a.json:
        say(json.dumps(payload, indent=1, sort_keys=True))
    else:
        s = payload["summary"]
        say(
            f"repo health, last {payload['window_days']} days: {s['gate_runs']} gate run(s), {s['failed_steps']}/{s['step_runs']} step run(s) failed, e2e {s['e2e_passed']}/{s['e2e_total']}, checkup {s['checkup_status']}, {s['open_prs']} open PR(s)"
        )
        for row in payload["steps"][:5]:
            say(f"  {row['step']:<14} {row['failed']}/{row['runs']} failed, {row['seconds_avg']} s on average")
    for p in written:
        say(f"wrote {p}")
    return 0


NO_PAGES = "no run pages yet: run scripts/checkup.sh, `careerbrew`, or `careerbrew review` (the run_pages setting must be on)"


def cmd_day_next(_a: argparse.Namespace) -> int:
    _say_stale_sources()
    from .day import planner

    nxt = planner.next_step()
    planner.log_suggestion(nxt)
    title = f" — {nxt['title']}" if nxt.get("title") and may_show_business() else ""
    say(f"next: {nxt['text']}{title}")
    if nxt.get("reason"):
        say(f"why: {nxt['reason']}")
    return 0


# ---------- learn (skills/careerbrew-learn-project) ----------
def _learn_specs(a: argparse.Namespace) -> list[tuple[str, str]]:
    return [(kind, spec) for kind in ("repo", "doc", "web", "context", "file") for spec in getattr(a, kind) or []]


def cmd_learn_list(_a: argparse.Namespace) -> int:
    from .learn import project

    names = project.projects()
    for name in names:
        st = project.status(name)
        say(f"{name}: {st['mastered']}/{st['concepts']} mastered, {st['due_reviews'] + st['due_retests']} due")
    say(f"{len(names)} learn project(s)." + ("" if names else " Start one: careerbrew learn start <name> --repo PATH"))
    return 0


def cmd_learn_start(a: argparse.Namespace) -> int:
    from .learn import project

    m = project.start(a.project, _learn_specs(a), consume=a.consume)
    fresh = m["freshness"]
    say(f"{a.project}: {len(m['sources'])} source(s) staged; changed: {', '.join(fresh['changed']) or 'none'}; unchanged: {len(fresh['unchanged'])}")
    say(f"next: write notes (careerbrew learn note {a.project} <section>/<note>.md --set FILE), then careerbrew learn concepts {a.project} FILE")
    return 0


def cmd_learn_note(a: argparse.Namespace) -> int:
    from .learn import project

    if a.set:
        src = Path(a.set)
        project.note(a.project, a.path, src.read_text(encoding="utf-8"))
        if a.consume:
            src.unlink()
        say(f"saved note {a.path}")
    else:
        say(project.note(a.project, a.path))
    return 0


def cmd_learn_concepts(a: argparse.Namespace) -> int:
    import yaml

    from .learn import project

    src = Path(a.file)
    rows = (yaml.safe_load(src.read_text(encoding="utf-8")) or {}).get("concepts", [])
    if a.check:
        problems = project.check_concepts(a.project, rows)
        for line in problems:
            say(f"  {line}")
        say(f"concepts check: {'pass' if not problems else f'{len(problems)} problem(s)'}")
        return 0 if not problems else 1
    out = project.import_concepts(a.project, rows)
    if a.consume:
        src.unlink()
    say(f"imported {out['concepts']} concept(s) in {out['sections']} section(s); each is a card with spaced reviews")
    return 0


def cmd_learn_next(a: argparse.Namespace) -> int:
    from .learn import project

    step = project.next_step(a.project)
    if a.json:
        say(json.dumps(step, indent=2))
    else:
        say(f"{step['mode']}: {step.get('title', '')} ({step['why']})".replace(":  (", ": ("))
        if "question" in step:
            say(f"ask at {step['level']}: {step['question']['prompt']}")
    return 0


def cmd_learn_answer(a: argparse.Namespace) -> int:
    from .learn import project

    row = project.answer(a.project, a.concept, a.result, a.confidence, a.level, mode=a.mode, kind=a.kind, seconds=a.seconds)
    flag = " misconception: high confidence and wrong; drill it first" if a.confidence == "high" and a.result == "wrong" else ""
    say(f"{a.concept}: {a.result}" + ("; mastered (retests at +1 and +7 days)" if row["mastered"] else "") + flag + ("; leech: forgotten many times, rewrite its note and questions" if row.get("leech") else ""))
    return 0


def cmd_learn_status(a: argparse.Namespace) -> int:
    from .learn import project

    st = project.status(a.project)
    if a.json:
        say(json.dumps(st, indent=2))
        return 0
    for name, s in st["sections"].items():
        say(f"  {s['known_pct']:3d}%  {name}  ({len(s['remaining'])} left)")
    say(
        f"{a.project}: {st['mastered']}/{st['concepts']} mastered · due: {st['due_reviews']} review(s), {st['due_retests']} retest(s)"
        f" · 7-day retest kept: {st['retest_retained_7d'] if st['retest_retained_7d'] is not None else 'n/a'}"
        f" · calibration (Brier): {st['brier'] if st['brier'] is not None else 'n/a'}"
    )
    if st["stale"]:
        say(f"changed sources to re-teach: {', '.join(st['stale'])}")
    return 0


def cmd_learn_replay_list(a: argparse.Namespace) -> int:
    from .learn import replay

    rows = replay.candidates(replay._repo(a.project, a.repo), a.limit)
    if a.json:
        say(json.dumps(rows, indent=2))
        return 0
    for r in rows:
        say(f"  #{r['pr']:<5} {len(r['tests'])} test file(s)  {r['title']}")
    say(f"{len(rows)} small PR(s); start one: careerbrew learn replay start {a.project} <pr>")
    return 0


def cmd_learn_replay_start(a: argparse.Namespace) -> int:
    from .learn import replay

    t = replay.start(a.project, a.pr, a.repo, a.python)
    say(f"PR #{t['pr']}: {t['title']}")
    say(f"code at {t['dir']} is from just before the PR; these tests now fail: {', '.join(t['tests'])}")
    say(f"make them pass, then: careerbrew learn replay check {a.project} {a.pr}")
    return 0


def cmd_learn_replay_check(a: argparse.Namespace) -> int:
    from .learn import replay

    r = replay.check(a.project, a.pr)
    say(f"PR #{a.pr}: {'pass' if r['passed'] else 'fail'} ({r['last']})")
    return 0 if r["passed"] else 1


def cmd_learn_replay_measure(a: argparse.Namespace) -> int:
    from .learn import replay

    r = replay.measure(Path(a.repo).expanduser().resolve(), a.limit, a.python)
    if a.json:
        say(json.dumps(r, indent=2))
        return 0
    for row in r["rows"]:
        say(f"  #{row['pr']:<5} {'yes' if row['runnable'] else 'no '}  {row['why']}")
    say(f"replay coverage: {r['runnable']}/{r['prs']} small PRs make a runnable task ({r['share']})")
    return 0


def cmd_learn_tour_add(a: argparse.Namespace) -> int:
    from .learn import tour

    src = Path(a.file)
    t = tour.add(a.project, a.name, json.loads(src.read_text(encoding="utf-8")))
    if a.consume:
        src.unlink()
    say(f"tour {a.name}: {len(t['steps'])} step(s), not yet approved; a maintainer runs: careerbrew learn tour approve {a.project} {a.name} --by NAME")
    return 0


def cmd_learn_tour_approve(a: argparse.Namespace) -> int:
    from .learn import tour

    t = tour.approve(a.project, a.name, a.by)
    say(f"tour {a.name} approved at {t['approved']['sha'][:12]}")
    return 0


def cmd_learn_tour_export(a: argparse.Namespace) -> int:
    from .learn import tour

    out, info = tour.export(a.project, a.name)
    say(f"tour {a.name}: {info['state']}, {info['faded']}/{info['steps']} step(s) faded to a prediction; open {out} with CodeTour: Open Tour File")
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="careerbrew", description="Private, local-first career assistant for programmers. With no command: start everything (watcher, web app, refresh) and show what matters now.")
    p.set_defaults(fn=cmd_start, cmd="start")
    p.add_argument("--no-watch", dest="no_watch", action="store_true", help="with no command: do not turn the day watcher on")
    p.add_argument("--no-web", dest="no_web", action="store_true", help="with no command: do not start the web app")
    p.add_argument("--no-refresh", dest="no_refresh", action="store_true", help="with no command: skip the first refresh")
    p.add_argument("--version", action="version", version=f"careerbrew {__version__}")
    sub = p.add_subparsers(dest="cmd")

    def add(parent, name, fn, help_text):
        sp = parent.add_parser(name, help=help_text)
        sp.set_defaults(fn=fn, story_cmd=name)
        return sp

    add(sub, "init", cmd_init, "create the private data folder from examples")
    mt = sub.add_parser("metrics", help="opt-in weekly aggregate metrics (numbers only)").add_subparsers(dest="sub", required=True)
    add(mt, "weekly", cmd_metrics_weekly, "write this week's numbers-only metrics file (when metrics_share is on); --dry-run shows it").add_argument("--dry-run", dest="dry_run", action="store_true")
    add(mt, "time-saved", cmd_metrics_time_saved, "minutes careerbrew saved you per week: your manual baseline minus your time (web app: up to copy, send, or accept; CLI: run time), per action (numbers only)").add_argument(
        "--json", action="store_true"
    )
    add(mt, "field", cmd_metrics_field, "this machine's real-use totals (runs, success rate, run time, time saved) under a salted-hash label; local only").add_argument("--json", action="store_true")
    tm = sub.add_parser("time", help="your manual time per action, for time saved").add_subparsers(dest="sub", required=True)
    sp = add(tm, "baseline", cmd_time, "record how many minutes an action takes you by hand (0 returns to the labeled estimate)")
    sp.add_argument("action", choices=["pm_update", "standup", "day_log", "day_plan", "evidence", "promo_doc", "job_scan", "inbox"])
    sp.add_argument("minutes")
    tl = sub.add_parser("telemetry", help="anonymous usage counts to PostHog EU (no content; needs a key)").add_subparsers(dest="sub", required=True)
    add(tl, "status", cmd_telemetry, "on or off and why, the endpoint, the random install id, and how many events wait")
    add(tl, "on", cmd_telemetry, "turn telemetry on (it sends only when a key is set: careerbrew secret set posthog_key)")
    add(tl, "off", cmd_telemetry, "turn telemetry off: nothing is sent")
    add(tl, "send", cmd_telemetry, "send the waiting events now; --dry-run prints the exact JSON and sends nothing").add_argument("--dry-run", dest="dry_run", action="store_true")
    sp = add(sub, "actions", cmd_actions, "every action, for agents and people: CLI command, arguments, and how the web app runs it")
    sp.add_argument("name", nargs="?", help="one action or group, e.g. day or day.send")
    sp.add_argument("--json", action="store_true")
    sp = add(sub, "start", cmd_start, "start everything (the default with no command): watcher, web app, refresh, then what matters now")
    sp.add_argument("--no-watch", dest="no_watch", action="store_true")
    sp.add_argument("--no-web", dest="no_web", action="store_true")
    sp.add_argument("--no-refresh", dest="no_refresh", action="store_true")
    add(sub, "checklist", cmd_checklist, "getting started: the first steps for a new user, each done or with the command that does it")
    cf = sub.add_parser("config", help="settings with defaults: show, set, unset (people and agents)").add_subparsers(dest="action")
    cf.add_parser("show", help="every setting, its value, and where it comes from")
    sp = cf.add_parser("set", help="validate and save a setting")
    sp.add_argument("key")
    sp.add_argument("value")
    cf.add_parser("unset", help="return a setting to its default").add_argument("key")
    sub.choices["config"].set_defaults(fn=cmd_config, story_cmd="config", action="show")
    for sp in cf.choices.values():
        sp.set_defaults(fn=cmd_config, story_cmd="config")
    sp = add(sub, "signals", cmd_signals, "what each of your decisions changed: ranks, the question budget, what plugins learn; reset <kind> <source> forgets one")
    sp.add_argument("action", nargs="?", choices=["reset"], help="reset: forget one signal's decisions so far")
    sp.add_argument("kind", nargs="?", help="with reset: the kind, as `careerbrew signals` names it (plugin, impact, competency...)")
    sp.add_argument("source", nargs="?", help="with reset: the source, as `careerbrew signals` names it (plugin:acme, github, project:billing...)")
    sp.add_argument("--json", action="store_true")
    add(sub, "checkup", cmd_checkup, "one pass/fail line per step: pull-only clone, venv, plugins, readiness, privacy, collect, promo coverage").add_argument("--offline", action="store_true", help="skip the network evidence collect")

    jobs = sub.add_parser("jobs", help="job-hunt mode").add_subparsers(dest="sub", required=True)
    add(jobs, "scan", cmd_jobs_scan, "fetch postings from your job sources").add_argument("--no-cache", action="store_true")
    sp = add(jobs, "choose", cmd_jobs_choose, "choose a posting you want, and its tier (careerbrew never chooses for you)")
    sp.add_argument("posting")
    sp.add_argument("tier", choices=["dream", "target", "practice"])
    add(jobs, "unchoose", cmd_jobs_unchoose, "remove a posting from your chosen jobs").add_argument("posting")
    add(jobs, "linkedin-import", cmd_jobs_linkedin_import, "import your LinkedIn data export (zip)").add_argument("zip")
    add(jobs, "interview-gaps", cmd_jobs_interview_gaps, "pipeline companies that still lack a sourced interview loop").add_argument("--top", type=int, default=20)
    add(jobs, "preset", cmd_jobs_preset, "write the remote-first job source and region preset").add_argument("--force", action="store_true")
    sp = add(jobs, "list", cmd_jobs_list, "postings that pass your criteria, by Want score")
    sp.add_argument("--top", type=int, default=20)
    sp.add_argument("--json", action="store_true")
    sp = add(jobs, "rate", cmd_jobs_rate, "rate a posting 1-5")
    sp.add_argument("id")
    sp.add_argument("rating", type=int)
    sp.add_argument("--tags", default="")
    add(jobs, "learn", cmd_jobs_learn, "refit Want weights to your ratings")
    add(jobs, "strategy", cmd_jobs_strategy, "build the season plan").add_argument("--json", action="store_true")
    sp = add(jobs, "log", cmd_jobs_log, "log an application stage")
    sp.add_argument("company")
    sp.add_argument("tier", choices=["practice", "target", "dream"])
    sp.add_argument("stage", choices=strategy.STAGES)

    cd = sub.add_parser("cards", help="flashcards with spaced repetition (FSRS)").add_subparsers(dest="sub", required=True)
    add(cd, "review", cmd_cards_review, "review due and new cards: recall, reveal, grade").add_argument("--count", type=int, default=20)
    sp = add(cd, "next", cmd_cards_next, "show the queue")
    sp.add_argument("--count", type=int, default=10)
    sp.add_argument("--json", action="store_true")
    sp = add(cd, "grade", cmd_cards_grade, "grade one card")
    sp.add_argument("id")
    sp.add_argument("rating", choices=["again", "hard", "good", "easy"])
    sp = add(cd, "add", cmd_cards_add, "add your own card")
    sp.add_argument("question")
    sp.add_argument("answer")
    sp.add_argument("--tags", default="")
    sp.add_argument("--deck", default="mine")
    add(cd, "stats", cmd_cards_stats, "due, new, retention, streak, topics")
    add(cd, "unsuspend", cmd_cards_unsuspend, "put a leech back in the queue").add_argument("id")
    sp = add(cd, "explain-queue", cmd_cards_explain_queue, "cards the public study deck should explain or rewrite next")
    sp.add_argument("--count", type=int, default=50)
    sp.add_argument("--topic", default="")
    sp.add_argument("--offset", type=int, default=0)
    sp.add_argument("--json", action="store_true", help="print the items for the card-explainer skill")
    sp = add(cd, "publish", cmd_cards_publish, "lint, privacy-check, and merge generated cards into knowledge/cards/")
    sp.add_argument("file")
    sp.add_argument("--keep", action="store_true", help="keep the input file")
    sp.add_argument("--dry-run", action="store_true", help="check the cards without writing anything")
    add(cd, "public-check", cmd_cards_public_check, "check every public card against the bar")
    ln = sub.add_parser("learn", help="learn a project: sources, cited concepts, mastery, spaced reviews (skills/careerbrew-learn-project)").add_subparsers(dest="sub", required=True)
    add(ln, "list", cmd_learn_list, "your learn projects with mastery and what is due")
    sp = add(ln, "start", cmd_learn_start, "stage sources: repos by path and commit, fetched text stored encrypted")
    sp.add_argument("project")
    sp.add_argument("--repo", action="append", help="a local checkout (repeatable)")
    sp.add_argument("--doc", action="append", help="LABEL:FILE, a doc you exported (repeatable)")
    sp.add_argument("--web", action="append", help="URL:FILE, a page you fetched (repeatable)")
    sp.add_argument("--context", action="append", help="LABEL:FILE, a PR, issue, or ticket summary (repeatable)")
    sp.add_argument("--file", action="append", help="LABEL:FILE, any other text file (repeatable)")
    sp.add_argument("--consume", action="store_true", help="delete each fetched file once it is stored encrypted")
    sp = add(ln, "note", cmd_learn_note, "read a study note, or save one with --set FILE")
    sp.add_argument("project")
    sp.add_argument("path", help="e.g. 01-overview/data-flow.md")
    sp.add_argument("--set", help="save this file as the note")
    sp.add_argument("--consume", action="store_true", help="delete the --set file once it is stored")
    sp = add(ln, "concepts", cmd_learn_concepts, "import concepts (sections, prerequisites, citations, questions); --check only checks")
    sp.add_argument("project")
    sp.add_argument("file", help="YAML with a `concepts:` list")
    sp.add_argument("--check", action="store_true")
    sp.add_argument("--consume", action="store_true", help="delete the file once it is imported")
    sp = add(ln, "next", cmd_learn_next, "the one thing to practise next: retest, review, pretest, teach, or quiz")
    sp.add_argument("project")
    sp.add_argument("--json", action="store_true")
    sp = add(ln, "answer", cmd_learn_answer, "record one answer: mastery, spaced review (FSRS), retests")
    sp.add_argument("project")
    sp.add_argument("concept")
    sp.add_argument("--result", required=True, choices=["correct", "partial", "wrong", "dontknow"])
    sp.add_argument("--confidence", required=True, choices=["low", "medium", "high"])
    sp.add_argument("--level", required=True, choices=["remember", "understand", "apply", "analyze", "evaluate", "create"], help="the question's Bloom level")
    sp.add_argument("--mode", default="quiz", choices=["pretest", "teach", "quiz", "recall", "review", "retest", "task"])
    sp.add_argument("--kind", default="open", choices=["mcq", "open", "task"], help="the question's format, kept in the log")
    sp.add_argument("--seconds", type=int)
    sp = add(ln, "status", cmd_learn_status, "mastery per section, what is due, 7-day retest score, calibration")
    sp.add_argument("project")
    sp.add_argument("--json", action="store_true")
    rp = ln.add_parser("replay", help="replay a real merged PR as a task its own tests grade").add_subparsers(dest="action", required=True)
    sp = add(rp, "list", cmd_learn_replay_list, "the newest small squash-merged PRs and their test files")
    sp.add_argument("project")
    sp.add_argument("--repo", help="default: the project's first staged repo")
    sp.add_argument("--limit", type=int, default=20)
    sp.add_argument("--json", action="store_true")
    sp = add(rp, "start", cmd_learn_replay_start, "a clone at the PR's parent with only its tests: make them pass")
    sp.add_argument("project")
    sp.add_argument("pr", type=int)
    sp.add_argument("--repo", help="default: the project's first staged repo")
    sp.add_argument("--python", help="the Python that runs the tests (default: careerbrew's)")
    sp = add(rp, "check", cmd_learn_replay_check, "run the PR's tests on your change")
    sp.add_argument("project")
    sp.add_argument("pr", type=int)
    sp = add(rp, "measure", cmd_learn_replay_measure, "how many of the last N small PRs make a runnable task (tests fail before, pass after)")
    sp.add_argument("--repo", required=True)
    sp.add_argument("--limit", type=int, default=50)
    sp.add_argument("--python", help="the Python that runs the tests (default: careerbrew's)")
    sp.add_argument("--json", action="store_true")
    tr = ln.add_parser("tour", help="code tours that fade as you learn, approved by a maintainer").add_subparsers(dest="action", required=True)
    sp = add(tr, "add", cmd_learn_tour_add, "add a tour: JSON with repo, title, and steps (file, line, description, concept)")
    sp.add_argument("project")
    sp.add_argument("name")
    sp.add_argument("file")
    sp.add_argument("--consume", action="store_true", help="delete the file once it is stored")
    sp = add(tr, "approve", cmd_learn_tour_approve, "a maintainer approves the tour at the repo's current commit")
    sp.add_argument("project")
    sp.add_argument("name")
    sp.add_argument("--by", required=True, help="who reviewed it")
    sp = add(tr, "export", cmd_learn_tour_export, "write a CodeTour file to out/tours/; mastered steps ask you to predict")
    sp.add_argument("project")
    sp.add_argument("name")
    gl = sub.add_parser("goals", help="your long-term goals and the upcoming goal (only you set them)").add_subparsers(dest="sub", required=True)
    add(gl, "show", cmd_goals_show, "your goals and what they focus on")
    sp = add(gl, "set", cmd_goals_set, "set the upcoming goal; today re-plans at once")
    sp.add_argument("title")
    sp.add_argument("--kind", default="custom", choices=["interview", "learn", "job-search", "promotion", "custom"])
    sp.add_argument("--date")
    sp.add_argument("--company")
    sp.add_argument("--topics", default="")
    sp.add_argument("--minutes", type=int, default=90)
    sp = add(gl, "add-long-term", cmd_goals_long_term, "add a long-term goal")
    sp.add_argument("text")
    sp.add_argument("--by")
    ap = sub.add_parser("approvals", help="actions waiting for your approval").add_subparsers(dest="sub", required=True)
    sp = add(ap, "request", cmd_approvals_request, "(for agents) ask the owner to approve an action")
    sp.add_argument("title")
    sp.add_argument("--detail", default="")
    sp.add_argument("--by", default="agent")
    add(ap, "list", cmd_approvals_list, "recent approval requests")
    add(ap, "guidance", cmd_approvals_guidance, "your notes on past decisions: standing guidance agents read before they ask again")
    add(ap, "show", cmd_approvals_show, "one request; exit 0 only when approved").add_argument("id")
    sp = add(ap, "decide", cmd_approvals_decide, "approve or reject (owner only)")
    sp.add_argument("id")
    sp.add_argument("decision", choices=["approved", "rejected"])
    sp.add_argument("--note", default="")
    tc = sub.add_parser("teach", help="teacher mode: one step at a time (best in the web app)").add_subparsers(dest="sub", required=True)
    add(tc, "next", cmd_teach_next, "the next step and why").add_argument("--json", action="store_true")
    add(tc, "improvements", cmd_teach_improvements, "predicted and written feedback, with proposed fixes")
    sp = add(tc, "feedback", cmd_teach_feedback, "tell the teacher what to improve")
    sp.add_argument("text")
    sp.add_argument("--step", default="")
    ex = sub.add_parser("exercises", help="coding exercises in curriculum order").add_subparsers(dest="sub", required=True)
    add(ex, "check", cmd_exercises_check, "every reference solution passes its tests (or check one YAML file)").add_argument("file", nargs="?")
    add(ex, "list", cmd_exercises_list, "the curriculum in order, with your progress")
    sp = add(ex, "sync", cmd_exercises_sync, "skip what you solved elsewhere: a public LeetCode profile (opt-in) or slugs you list")
    sp.add_argument("--leetcode", dest="username", default="")
    sp.add_argument("solved", nargs="*", help="LeetCode slugs or exercise ids you already solved")
    sp = add(ex, "submit", cmd_exercises_submit, "run and grade a solution file")
    sp.add_argument("id")
    sp.add_argument("file")
    sp.add_argument("--seconds", type=int)
    im = sub.add_parser("import", help="import from other tools").add_subparsers(dest="sub", required=True)
    sp = add(im, "notion", cmd_import_notion, "import a Notion career bundle (written by the notion-import skill)")
    sp.add_argument("--dir", help="bundle folder (default <data root>/imports/notion)")
    sp.add_argument("--check", action="store_true", help="compare the bundle with what is imported")
    lb = sub.add_parser("library", help="imported notes and guides").add_subparsers(dest="sub", required=True)
    add(lb, "list", cmd_library_list, "list library pages")
    add(lb, "show", cmd_library_show, "print one page").add_argument("id")
    drill = sub.add_parser("drill", help="interview tutor").add_subparsers(dest="sub", required=True)
    add(drill, "next", cmd_drill_next, "show the next item (due reviews first)").add_argument("--json", action="store_true")
    sp = add(drill, "answer", cmd_drill_answer, "grade an attempt by rubric criteria met")
    sp.add_argument("item")
    sp.add_argument("--met", default="")
    sp.add_argument("--seconds", type=int)
    sp = add(drill, "speak", cmd_drill_speak, "answer by voice (local speech-to-text) or typed text; graded by the rubric")
    sp.add_argument("item")
    sp.add_argument("--audio", help="recorded answer (wav, or any format ffmpeg reads)")
    sp.add_argument("--text", help="typed answer (the fallback when there is no microphone)")
    sp.add_argument("--aloud", action="store_true", help="read the question aloud with the system voice")
    sp.add_argument("--seconds", type=int)
    add(drill, "session", cmd_drill_session, "interactive drill loop").add_argument("--count", type=int, default=5)
    add(drill, "progress", cmd_drill_progress, "write the progress chart")
    add(drill, "list", cmd_drill_list, "list items")

    work = sub.add_parser("work", help="current-job mode").add_subparsers(dest="sub", required=True)
    add(work, "collect", cmd_work_collect, "collect evidence from git, gh, and plugins")
    pj = work.add_parser("project", help="track a project's sources and when each was last read").add_subparsers(dest="action", required=True)
    sp = pj.add_parser("add", help="create a project record")
    sp.add_argument("name")
    sp.add_argument("--memory", help="path to the agent's notes for this project")
    sp.add_argument("--stale-after", dest="stale_after", default="7d", help="7d or 2w (default 7d)")
    edit = pj.add_parser("set", help="change a project's title, group, parent, or links (the value none clears one)")
    edit.add_argument("name")
    for target in (sp, edit):
        target.add_argument("--title", help="one line: what the project is")
        target.add_argument("--group", help="a label such as a team, a goal list, or a quarter's cutline")
        target.add_argument("--parent", help="the project this one belongs to")
        target.add_argument("--link", action="extend", nargs="+", help="a related project (repeat for more)")
        target.add_argument("--kind", help="prep: it prepares you for an event (an interview, a meeting, a demo, a talk) and is done only after its readiness gate")
        target.add_argument("--event-on", dest="event_on", help="a prep project's event day, YYYY-MM-DD: that day's plan starts with its open gates")
    edit.add_argument("--unlink", action="extend", nargs="+", help="drop a link")
    edit.add_argument("--quiet-until", help="YYYY-MM-DD or none")
    sp = pj.add_parser("import", help="add or update projects from a sheet: a CSV or JSON file, or a Google Sheets link")
    sp.add_argument("source", help="a .csv or .json file with columns name, title, group, parent, links, trackers (recommended: a CSV download); or a Google Sheets link only when the sheet's owner and your company policy allow it")
    sp.add_argument("--dry-run", dest="dry_run", action="store_true", help="check every row and show what would change; write nothing")
    sp = pj.add_parser("source", help="add a source: doc, tracker, repo, chat, memory, or url")
    sp.add_argument("name")
    sp.add_argument("kind")
    sp.add_argument("ref")
    sp = pj.add_parser("check", help="stamp checked_at now on one source (by ref) or all")
    sp.add_argument("name")
    sp.add_argument("ref", nargs="?")
    sp.add_argument("--status", help="one line: what the source says now")
    sp.add_argument("--since", action="store_true", help="stamp nothing: list which sources changed since their last check, so you read only those")
    sp = pj.add_parser("reference", help="mark a source reference-only: never read, never stale (links that open a sign-in page are references already)")
    sp.add_argument("name")
    sp.add_argument("ref")
    sp.add_argument("--off", action="store_true", help="track it again: it goes stale after the project's limit")
    sp = pj.add_parser("absorb", help="add a link, file, task id, or #channel to a project and note what it says")
    sp.add_argument("first", help="the project name, or the thing to absorb (the project is then matched)")
    sp.add_argument("second", nargs="?", help="the thing to absorb when a project name comes first")
    sp = pj.add_parser("memory", help="set the project's memory path (where absorb and checkup write notes)")
    sp.add_argument("name")
    sp.add_argument("path")
    sp = pj.add_parser("rules", help="read a project's standing session rules")
    sp.add_argument("name")
    sp.add_argument("--json", action="store_true", help="return the rules as JSON where business output is allowed")
    sp = pj.add_parser("rules-set", help="replace or clear a project's standing session rules")
    sp.add_argument("name")
    change = sp.add_mutually_exclusive_group(required=True)
    change.add_argument("--text", help="replace the rules with one line")
    change.add_argument("--clear", action="store_true", help="remove the rules")
    sp.add_argument("--yes", action="store_true", help="confirm replacement")
    pj.add_parser("sync", help="read each changed project memory file into its project (links, trackers, status), and write matching work items there as dated notes (the watcher does it each cycle)")
    sp = pj.add_parser("next", help="link a work task as a next step")
    sp.add_argument("name")
    sp.add_argument("task")
    sp = pj.add_parser("list", help="one line per active project: stale sources first; --json gives an agent each project's sources to watch")
    scope = sp.add_mutually_exclusive_group()
    scope.add_argument("--active", action="store_true", help="only the active projects (the default)")
    scope.add_argument("--all", action="store_true", help="archived projects too")
    sp.add_argument("--json", action="store_true", help="each project with its sources, last check, and latest update, as JSON")
    sp = pj.add_parser("archive", help="stop watching a project: off the list and the watcher, never stale; the record and notes stay (a prep project only once ready)")
    sp.add_argument("name")
    sp.add_argument("--force", action="store_true", help="archive a prep project with open gates (the event was cancelled)")
    sp = pj.add_parser("gate", help="mark a prep project's gate done: tool (tried it hands-on), links <ref>... (you read these links), or questions (your own questions are ready)")
    sp.add_argument("name")
    sp.add_argument("gate", help="tool, links, or questions (the quiz passes with a clean quiz)")
    sp.add_argument("refs", nargs="*", help="links only: each source link you read yourself")
    sp.add_argument("--undo", action="store_true", help="open the gate again")
    sp = pj.add_parser("quiz", help="store a prep project's quiz result, counts only: questions asked and missed (misses are re-quizzed)")
    sp.add_argument("name")
    sp.add_argument("--asked", type=int, required=True, help="how many questions the quiz asked")
    sp.add_argument("--missed", type=int, required=True, help="how many of them were missed")
    pj.add_parser("unarchive", help="watch an archived project again").add_argument("name")
    sp = pj.add_parser("remove", help="delete a project's record (needs --yes); its memory file and tasks stay")
    sp.add_argument("name")
    sp.add_argument("--yes", action="store_true", help="confirm the delete")
    sp = pj.add_parser("update", help="note one project change or approval (the Work view shows the latest)")
    sp.add_argument("name")
    sp.add_argument("kind", help="company, project, team, other, or approval")
    sp.add_argument("text", help="one line: what changed")
    sp = pj.add_parser("explain", help="write a layered, cited explanation outline, or --check a filled one")
    sp.add_argument("name")
    depth = sp.add_mutually_exclusive_group()
    depth.add_argument("--short", action="store_true", help="short depth: one-minute version, containers, codemap, gotchas, read next, sources")
    depth.add_argument("--deep", action="store_true", help="deep mode: adds a cited 'How it runs' section with the commands you ran")
    sp.add_argument("--check", help="run the checker on a filled doc; exit 1 on any problem")
    sp = pj.add_parser("prd", help="write a PRD outline (requirements, decisions, estimates in dev weeks), or --check a filled one")
    sp.add_argument("name")
    sp.add_argument("--slot", type=float, help="the plan slot in dev weeks; the checker asks for a pushback when the estimate is bigger")
    sp.add_argument("--check", help="run the PRD checker on a filled doc; exit 1 on any problem")
    pj.add_parser("show", help="sources stalest first, then next steps").add_argument("name")
    for sp in pj.choices.values():
        sp.set_defaults(fn=cmd_work_project, story_cmd="project")
    wp = work.add_parser("priority", help="rank your projects: the PM update, the plan, and Next step follow it").add_subparsers(dest="action")
    wp.add_parser("show", help="the ranked projects, their patterns, and what each matches now")
    sp = wp.add_parser("set", help="add a project or move it to a rank; --match sets the globs it owns (repos, ticket keys, titles)")
    sp.add_argument("project")
    sp.add_argument("--rank", type=int, help="1 is the top project (default: keep its rank, or last for a new one)")
    sp.add_argument("--match", action="extend", nargs="+", help="a glob such as */agentbrew, PROJ-*, or *invoice* (repeat for more; default: keep them, or the name)")
    wp.add_parser("remove", help="drop a project from the ranking").add_argument("project")
    work.choices["priority"].set_defaults(fn=cmd_work_priority, story_cmd="priority", action="show")
    for sp in wp.choices.values():
        sp.set_defaults(fn=cmd_work_priority, story_cmd="priority")
    fd = work.add_parser("feed", help="one local store for Slack asks, PagerDuty pages, Outlook mail and meetings: triaged by rule, personal kept out").add_subparsers(dest="action")
    sp = fd.add_parser("show", help="counts by triage (act, fyi, noise, mine), open asks, personal; --show lists titles to your terminal")
    sp.add_argument("--show", action="store_true")
    sp.add_argument("--open", action="store_true", help="only asks with no reply or acknowledge from you yet")
    sp.add_argument("--triage", choices=("act", "fyi", "noise", "mine"))
    sp.add_argument("--source", help="slack, pagerduty, outlook, or a plugin's source name")
    sp.add_argument("--personal", action="store_true", help="also list personal events")
    sp.add_argument("--top", type=int, default=50)
    fd.add_parser("pull", help="read Slack, PagerDuty, and Outlook into the feed now (the watcher does it every 15 minutes)")
    sp = fd.add_parser("add", help="add events from a JSON-lines file (- for stdin): id, at, title, and source, kind, actor, to_me, text, personal")
    sp.add_argument("file")
    sp.add_argument("--plugin", help="the installed plugin that sends them (plugin remove --purge-data deletes them)")
    sp.add_argument("--source", help="for a raw tool reply saved as returned ({tickets: [...]} or {messages: [...]}): its feed source (default jira or mail)")
    sp = fd.add_parser("transcript", help="store the feed tools' replies from an agent session transcript (.jsonl); fails on a mapped source that stored nothing (feed_tool_sources)")
    sp.add_argument("file", help="Claude/Codex JSONL")
    sp.add_argument("--since", help="this cycle only: an ISO time, or a duration back from now such as 30m or 2h (default: the whole transcript)")
    sp = fd.add_parser("cycle", help="end a feed batch: report missing or invalid required acquisition coverage, then begin a new batch")
    sp.add_argument("--require", help="additional required sources, comma-separated; configured tool and required live sources always apply")
    sp.add_argument("--json", action="store_true", help="the required, stored, and not_stored source names as JSON")
    sp = fd.add_parser("mark", help="mark one event personal (kept out of evidence, the day log, project memory, and the PM update) or work; your mark wins over rules")
    sp.add_argument("id")
    sp.add_argument("value", choices=("personal", "work", "not-ask", "done", "ask"), help="personal or work; for an ask: not-ask (it never asked you), done (handled elsewhere), or ask (open again)")
    fd.add_parser("rules", help="the triage rules in the order they run: yours (work/feed-rules.yaml), then the built-in ones")
    fd.add_parser("retriage", help="run the rules again over every event whose original is still kept (after a rules or team change)")
    sp = fd.add_parser("links", help="kept events with links, and the links not researched yet (--pending: only those)")
    sp.add_argument("--pending", action="store_true")
    sp.add_argument("--top", type=int, default=50)
    sp = fd.add_parser("research", help="store what each link of one event says; refused unless every link has a summary, or opened: false and why")
    sp.add_argument("id")
    sp.add_argument("file", help="a JSON list (- for stdin): {url, summary} or {url, opened: false, reason} per link")
    fd.add_parser("open", help="one event's original text, to your terminal").add_argument("id")
    work.choices["feed"].set_defaults(fn=cmd_work_feed, story_cmd="feed", action="show", show=False, open=False, triage=None, source=None, personal=False, top=50)
    for sp in fd.choices.values():
        sp.set_defaults(fn=cmd_work_feed, story_cmd="feed")
    lv = work.add_parser("live", help="the live feed: pull every source, store, triage, a verdict per event, then mark the cycle (missing, invalid or stale required acquisition fails it)").add_subparsers(dest="action")
    sp = lv.add_parser("start", help="mark the live feed running and list what each source pulls; --sources saves a new source config first")
    sp.add_argument("--sources", help="a source config, YAML or JSON (- for stdin): sources: [{name, kind, required, adapter: file|connector, inbox|connector, pull}]")
    lv.add_parser("cycle", help="one cycle: pull, store, triage, verdict per event, next actions; exits 1 when a required source is not healthy (not_stored)")
    lv.add_parser("status", help="running or stopped, the last cycle, each source's cursor, and the next actions").add_argument("--json", action="store_true")
    lv.add_parser("stop", help="stop the live feed; cursors are kept, so the next start continues")
    work.choices["live"].set_defaults(fn=cmd_work_live, story_cmd="live", action="status", json=False)
    for sp in lv.choices.values():
        sp.set_defaults(fn=cmd_work_live, story_cmd="live")
    pm = work.add_parser("promise", help='promises you made in sent messages ("I\'ll look into it"), tracked as projects until done').add_subparsers(dest="action")
    sp = pm.add_parser("list", help="open promises: overdue first, then by due day, with the next step (research, draft, status)")
    sp.add_argument("--all", action="store_true", help="also the closed ones")
    sp.add_argument("--json", action="store_true")
    pm.add_parser("scan", help="look for promises in your sent messages still kept in the work feed (new ones are tracked as they arrive)")
    sp = pm.add_parser("add", help="track a promise made outside the feed (a meeting, a call)")
    sp.add_argument("text", help="what you promised, one line")
    sp.add_argument("--to", help="who you promised it to")
    sp.add_argument("--due", help="the due day, YYYY-MM-DD")
    sp.add_argument("--link", help="where it was made (an http(s) link)")
    sp.add_argument("--parent", help="the project it belongs to")
    pm.add_parser("outcomes", help="show verified PM promise outcome proposals").add_argument("--json", action="store_true")
    sp = pm.add_parser("accept", help="accept one current PM promise proposal (owner only, needs --yes)")
    sp.add_argument("proposal_id")
    sp.add_argument("--yes", action="store_true")
    sp = pm.add_parser("stats", help="count promise outcomes in an inclusive creation-day window")
    sp.add_argument("--days", type=int, required=True)
    sp.add_argument("--json", action="store_true")
    pm.add_parser("done", help="close a promise you kept: it leaves Commitments and the handoff").add_argument("name")
    pm.add_parser("dropped", help="close a promise that was not one (detected by mistake) or no longer stands").add_argument("name")
    sp = pm.add_parser("step", help="record one step of the work on a promise: research, draft (the follow-up), or status")
    sp.add_argument("name")
    sp.add_argument("step", choices=("research", "draft", "status"))
    sp.add_argument("text", help="one line: what was done")
    work.choices["promise"].set_defaults(fn=cmd_work_promise, story_cmd="promise", action="list", all=False, json=False)
    for sp in pm.choices.values():
        sp.set_defaults(fn=cmd_work_promise, story_cmd="promise")
    ag = work.add_parser("agent", help="background agents' progress, one event per step while they work; Work's Agent activity shows it live").add_subparsers(dest="action")
    sp = ag.add_parser("event", help="report one step now (never batch at the end of a cycle): Agent activity shows it within seconds")
    sp.add_argument("agent", help="the agent's name, for example review-bot")
    sp.add_argument("title", help="one line: what the agent did or does now (up to 200 characters)")
    sp.add_argument("--status", choices=("running", "done", "failed"), default="running", help="running (default) while it works; done or failed ends the run")
    sp.add_argument("--project", default="", help="the project it works for; Agent activity groups by it")
    sp.add_argument("--url", default="", help="an http(s) link to what it works on (a PR, a ticket, a run page)")
    sp.add_argument("--run", default="", help="the run id when one agent runs several jobs at once (default: the agent's name)")
    sp.add_argument("--session", default="", help="a project session's id (`work session open`): sets the run and the project")
    sp = ag.add_parser("show", help="each background agent's run: status, project, and last event (titles and links only to your terminal)")
    sp.add_argument("--json", action="store_true", help="the runs as JSON (titles and links only where business data may show)")
    work.choices["agent"].set_defaults(fn=cmd_work_agent, story_cmd="agent", action="show", json=False)
    for sp in ag.choices.values():
        sp.set_defaults(fn=cmd_work_agent, story_cmd="agent")
    ss = work.add_parser("session", help="project sessions: one long-lived agent session per project, reused all day, writing through careerbrew (issue #274)").add_subparsers(dest="action")
    sp = ss.add_parser("open", help="record a project's session, or get its live one (one per project; at most work_session_cap live)")
    sp.add_argument("project", help="the project's name (careerbrew work project list)")
    sp.add_argument("--id", help="the agent tool's own id for the new session (not needed to get the live one)")
    sp.add_argument("--json", action="store_true")
    sp = ss.add_parser("list", help="every open project session: state, last agent event, claims, open questions, handoff (projects only to your terminal)")
    sp.add_argument("--all", action="store_true", help="also the archived (ended) sessions")
    sp.add_argument("--json", action="store_true")
    sp = ss.add_parser("ask", help="save a session's question for the main session's one batched ask (a session never asks the user)")
    sp.add_argument("id", help="the session's id")
    sp.add_argument("text", help="the ask, one line (at most 200 characters)")
    sp.add_argument("--option", action="append", default=[], help="one answer the user can pick (repeat it, at most 6)")
    sp.add_argument("--pick", help="your pick: one of the --option values")
    sp.add_argument("--recheck", help="a command (exit 0 means still open) or an http(s):// link an agent checks")
    sp = ss.add_parser("handoff", help="write the project's end-of-day handoff (one per project; a new one replaces it)")
    sp.add_argument("id", help="the session's id (or its project)")
    sp.add_argument("text", help="one line: where the project stands and the next step")
    sp.add_argument("--link", help="an http(s) link to the detail")
    sp = ss.add_parser("end", help="archive a session that wrote its handoff and release its claims (exit 1 while one has no handoff)")
    sp.add_argument("id", nargs="?", default="", help="the session's id (or its project)")
    sp.add_argument("--all", action="store_true", help="every open session (end of day)")
    sp.add_argument("--json", action="store_true")
    work.choices["session"].set_defaults(fn=cmd_work_session, story_cmd="session", action="list", all=False, json=False)
    for sp in ss.choices.values():
        sp.set_defaults(fn=cmd_work_session, story_cmd="session")
    cl = work.add_parser("claim", help="claims: a project session's expiring hold on a shared thread or draft; other sessions skip a claimed one (issue #274)").add_subparsers(dest="action")
    for name, text in (("take", "claim a resource before writing it, or renew your claim (exit 1: another session holds it, so skip it)"), ("release", "drop your claim when the write is done")):
        sp = cl.add_parser(name, help=text)
        sp.add_argument("resource", help="named like a step's uses: entry, for example slack-draft:C123/1700000000.000100")
        sp.add_argument("--session", required=True, help="the session's id")
        if name == "take":
            sp.add_argument("--minutes", type=int, help="how long it lasts (default: work_claim_minutes)")
    sp = cl.add_parser("list", help="every claim held now: resource, session, and until when (resources only to your terminal)")
    sp.add_argument("--json", action="store_true")
    work.choices["claim"].set_defaults(fn=cmd_work_claim, story_cmd="claim", action="list", json=False)
    for sp in cl.choices.values():
        sp.set_defaults(fn=cmd_work_claim, story_cmd="claim")
    tm = work.add_parser("team", help="your team roster: teammates' work-feed events are worth a look, never noise").add_subparsers(dest="action")
    sp = tm.add_parser("add", help="add a teammate, or fill in ids of one already on the roster")
    sp.add_argument("name")
    sp.add_argument("--chat", help="chat user id (Slack), such as U0SAM")
    sp.add_argument("--email")
    sp.add_argument("--login", help="code-host login, such as samlee")
    tm.add_parser("remove", help="take a teammate off the roster").add_argument("name")
    tm.add_parser("list", help="the roster: names to your terminal, ids as yes or no to agents").add_argument("--json", action="store_true")
    sp = tm.add_parser("feed", help="your teammates' kept feed events, by project (none: no project matches), with what each link says")
    sp.add_argument("--days", type=int, default=7)
    sp.add_argument("--top", type=int, default=50)
    work.choices["team"].set_defaults(fn=cmd_work_team, story_cmd="team", action="list", json=False)
    for sp in tm.choices.values():
        sp.set_defaults(fn=cmd_work_team, story_cmd="team")
    sp = add(work, "standup", cmd_work_standup, "what you did on each row you report on since the last standup: one page (out/standup.html) and the Standup view")
    sp.add_argument("--rows", help="the rows to report on, a JSON or YAML file (- for stdin): a list of {title, status, owner, keys, project, repos, aliases}; kept for later runs; an empty list [] clears them")
    sp.add_argument(
        "--talk", help="talking points an agent wrote, a JSON or YAML file (- for stdin): {row title: string or [bullet, ...]}; 1 to 5 bullets, at most 200 characters each, replace that row's generated talking points until the next standup"
    )
    sp.add_argument("--extra", help="actions careerbrew does not collect (Slack, meetings, mail), a JSON or YAML file (- for stdin): a list of {title, at, kind, url, row}; they join the row's evidence until the next standup")
    sp.add_argument(
        "--proof",
        help="a live proof check an agent made right before the build, a JSON or YAML file (- for stdin): {row title: {verdict: pass|fail, proofs: [{source, url, checked_at, quote}], reason}}; with it, a row shows only when its check passes and its proofs are fresh (standup_proof_minutes)",
    )
    sp.add_argument("--show", action="store_true", help="print each row's talking point, and each failed row's title and reason, to your terminal")
    sp = add(work, "review-ask", cmd_work_review_ask, "draft a review ask for a PR that names enough people for its required approvals (never fewer than 2; group handles do not count), each with what to review")
    sp.add_argument("url", help="the pull request link, https://<host>/<owner>/<repo>/pull/<number>")
    sp.add_argument("--reviewer", action="append", default=[], help="a person to name first, by login (repeatable)")
    sp.add_argument("--check", help="check a draft you wrote instead, a text file (- for stdin): refused when it names too few people")
    add(work, "focus", cmd_work_focus, "the most efficient action now, today, this week, month, quarter, year, and at this company, with why")
    sp = add(work, "note", cmd_work_note, "record evidence no tool sees")
    sp.add_argument("title")
    sp.add_argument("--tags", default="")
    sp.add_argument("--impact", default="")
    for name, fn, h in (("promo", cmd_work_promo, "write the promotion doc"), ("gaps", cmd_work_gaps, "what is missing, by priority"), ("plan", cmd_work_plan, "plan today's work")):
        sp = add(work, name, fn, h)
        sp.add_argument("--ladder")
        sp.add_argument("--level")
        if name == "promo":
            sp.add_argument("--show", action="store_true")
        if name == "plan":
            sp.add_argument("--hours", type=float, default=6.0)
    sp = add(work, "linkedin", cmd_work_linkedin, "public-safe LinkedIn Experience block from your results")
    sp.add_argument("--role", help='e.g. "Senior Software Engineer, Payments"')
    sp.add_argument("--max", type=int, default=6)
    sp.add_argument("--since", help="only evidence on or after YYYY-MM-DD")
    sp.add_argument("--show", action="store_true")
    sp = add(work, "task-set", cmd_work_task_set, "change a task's impact, urgency, or hours (an accepted task starts at 3, 3, 1h)")
    sp.add_argument("task")
    sp.add_argument("--impact", type=int, help="1 to 5")
    sp.add_argument("--urgency", type=int, help="1 to 5")
    sp.add_argument("--hours", type=float, help="your estimate in hours")
    sp = add(work, "impact", cmd_work_impact, "a task's impact as work happens: expected at the start, measured with evidence at delivery (issue #214)")
    sp.add_argument("task")
    sp.add_argument("--expect", default="", help="at the start: the hypothesis, what should change")
    sp.add_argument("--who", default="", help="at the start: who benefits")
    sp.add_argument("--metric", default="", help="at the start: the metric that will show it")
    sp.add_argument("--result", default="", help="at delivery: the measured result, a number when you have one")
    sp.add_argument("--evidence", action="append", default=[], help="at delivery: a link to a PR, dashboard, or message (repeatable)")
    sp.add_argument("--not-sure", dest="not_sure", action="store_true", help="the result cannot be checked: it is kept as 'Not sure: ...'")
    sp = add(work, "task", cmd_work_task, "add a task")
    sp.add_argument("title")
    sp.add_argument("--impact", type=int, default=3)
    sp.add_argument("--urgency", type=int, default=3)
    sp.add_argument("--hours", type=float, default=2.0)
    sp.add_argument("--competencies", default="")
    sp.add_argument("--due")
    sp.add_argument("--source", choices=("user", "agent"), help="who adds it; agents must pass agent (default: user in a terminal, agent otherwise)")
    sp.add_argument("--why", help="why it was added (required for agents); URLs in it become links in the web app")
    sp.add_argument("--link", action="append", help="a full https:// link to what the task needs (a file, a ticket, a channel), or LABEL=URL; repeat for more")
    sp = add(work, "done", cmd_work_done, "mark a task done with actual hours")
    sp.add_argument("id")
    sp.add_argument("hours", type=float)

    ladder = sub.add_parser("ladder", help="promotion ladders").add_subparsers(dest="sub", required=True)
    add(ladder, "list", cmd_ladder_list, "list ladders")
    add(ladder, "import", cmd_ladder_import, "validate and import a ladder YAML").add_argument("file")
    sp = add(ladder, "levels", cmd_ladder_levels, "a level's dimensions: description and looks-like examples")
    sp.add_argument("--ladder", help="ladder id (default: setting `ladder`)")
    sp.add_argument("--level", help="level id (default: setting `target_level`)")
    sp.add_argument("--json", action="store_true")

    dt = sub.add_parser("data", help="encryption at rest for the data root").add_subparsers(dest="sub", required=True)
    add(dt, "status", cmd_data_status, "is the data root encrypted, and how much of it")
    add(dt, "encrypt", cmd_data_encrypt, "encrypt every plaintext data file and turn encryption on")
    add(dt, "decrypt", cmd_data_decrypt, "write every data file as plaintext and turn encryption off").add_argument("--yes", action="store_true")
    add(dt, "edit", cmd_data_edit, "edit a data file (e.g. profile.yaml) in $EDITOR; it stays encrypted on disk").add_argument("file")
    sp = add(dt, "set", cmd_data_set, "set profile name or employer without an editor; do not use for secrets")
    sp.add_argument("file", help="profile.yaml")
    sp.add_argument("--name", help="your name")
    sp.add_argument("--employer", help="your employer")
    sp = add(dt, "show", cmd_data_show, "print a data file decrypted without changing it; --keys prints only its structure (for agents)")
    sp.add_argument("file")
    sp.add_argument("--keys", action="store_true", help="key names and value types only, never a value")

    bk = sub.add_parser("backup", help="encrypted backups").add_subparsers(dest="sub", required=True)
    add(bk, "create", cmd_backup_create, "write and verify an encrypted backup")
    add(bk, "verify", cmd_backup_verify, "verify a backup (latest by default)").add_argument("file", nargs="?")
    sp = add(bk, "restore", cmd_backup_restore, "restore a backup into a folder")
    sp.add_argument("file")
    sp.add_argument("dest")

    pl = sub.add_parser("plugin", help="corporate plugins").add_subparsers(dest="sub", required=True)
    sp = add(pl, "add", cmd_plugin_add, "install a plugin from a folder or git URL")
    sp.add_argument("source")
    sp.add_argument("--allow-exec", action="store_true")
    sp = add(pl, "remove", cmd_plugin_remove, "uninstall a plugin")
    sp.add_argument("name")
    sp.add_argument("--purge-data", action="store_true")
    add(pl, "list", cmd_plugin_list, "list plugins and the merged policy")
    add(pl, "refresh", cmd_plugin_refresh, "reinstall every plugin from its folder or URL after you edit it")
    sp = add(pl, "new", cmd_plugin_new, "create, install, and connect a plugin for a new job in one step")
    sp.add_argument("company")
    sp.add_argument("--dir", help="where to create it (default ~/careerbrew-plugins/careerbrew-<company>)")
    sp.add_argument("--connect", help="comma list: jira,github,slack,gdocs,zoom")
    sp.add_argument("--jira-url", dest="jira_url")
    sp.add_argument("--email")
    sp.add_argument("--github-user", dest="github_user")
    sp.add_argument("--github-api", dest="github_api", help="GitHub Enterprise API base, e.g. https://github.example.com/api/v3")
    sp.add_argument("--no-prompt", dest="no_prompt", action="store_true")
    add(pl, "verify", cmd_plugin_verify, "detect changed plugin files")

    pv = sub.add_parser("privacy", help="leak checks").add_subparsers(dest="sub", required=True)
    sp = add(pv, "scan", cmd_privacy_scan, "scan tracked/staged files (or a draft you are about to post, with --file) for sensitive terms")
    sp.add_argument("--staged", action="store_true")
    sp.add_argument("--repo")
    sp.add_argument("--file", dest="files", action="append", help="scan this draft instead of the repo (repeatable): terms, private/ paths, the data root")
    sp.add_argument("--show", action="store_true", help="also print each matched text with its line, for the owner only")

    sp = add(sub, "connect", cmd_connect, "register a work-tool connector (token goes to the Keychain)")
    sp.add_argument("kind", choices=connectors.KINDS)
    sp.add_argument("--ref", help="use an existing secret reference: keychain:<svc> | op://vault/item/field | env:NAME")
    sp.add_argument("--base-url", dest="base_url")
    sp.add_argument("--email")
    sp.add_argument("--user")
    sp.add_argument("--api")
    sp.add_argument("--days", type=int)
    sp.add_argument("--no-verify", dest="no_verify", action="store_true", help="save without the who-am-I check")
    sp.add_argument("--paste", action="store_true", help="outlook: paste a manual Graph token (it expires after about an hour) instead of signing in with a device code")
    sp.add_argument("--client-id", dest="client_id", help="outlook: the app registration to sign in with (default: the outlook_client_id setting, then a plugin's connector_defaults)")
    sp.add_argument("--tenant", help="outlook: the Microsoft tenant (default: the outlook_tenant setting, then a plugin's, then organizations)")
    sp = add(sub, "interview", cmd_interview, "the interview loop for a company: every step, tips, traps, sources")
    sp.add_argument("company")
    sp.add_argument("--level", type=int, default=3)
    sp.add_argument("--show", action="store_true")
    sp.add_argument("--no-sources", dest="no_sources", action="store_true")
    sp = add(sub, "open", cmd_open, "open the web app in your default browser (starts it if needed; never prints the token)")
    sp.add_argument("--view", default="today", help="view to open, for example today, week, dashboard")
    sp.add_argument("--report", help="open a run page instead: latest, a run name such as review, or a page id")
    rp = sub.add_parser("report", help="run pages: what each long run did, its blockers, and what to do next").add_subparsers(dest="sub", required=True)
    sp = add(rp, "latest", cmd_report, "the newest run page: its path, status, blockers, and next commands (--json: counts, ids, statuses, and commands only, for agents)")
    sp.add_argument("--run", help="only this run: repo-checkup, start, review, work-collect, day-plan, checkup, machine-ready, machine-new-user, or plugin-new")
    sp.add_argument("--json", action="store_true")
    sp = add(rp, "list", cmd_report, "every kept run page, newest first")
    sp.add_argument("--top", type=int, default=20)
    sp.add_argument("--json", action="store_true")
    sp = add(rp, "public", cmd_report_public, "repo health for the shared Dashboard: gate, e2e, and checkup numbers only, allowlisted (--json for agents; --out DIR writes its row files)")
    sp.add_argument("--days", type=int, default=28, help="the window, in days ending today (1 to 366)")
    sp.add_argument("--json", action="store_true")
    sp.add_argument("--out", help="write summary.json, steps.json, and daily.json for the repo health Dashboard here")
    add(sub, "serve", cmd_serve, "open the local web app (127.0.0.1 only)").add_argument("--port", type=int, default=8765)
    wb = sub.add_parser("web", help="the web app's sign-in: one token that lasts across restarts").add_subparsers(dest="sub", required=True)
    sp = add(wb, "rotate-token", cmd_web_rotate_token, "sign out every web app tab and bookmark now with a new token; `careerbrew open` signs you in again (never prints the token)")
    sp.add_argument("--yes", action="store_true", help="really sign out every open tab")
    add(sub, "changes", cmd_changes, "detect ladder, plugin, company, and source changes and pivot").add_argument("--fetch", action="store_true", help="also re-fetch pipeline companies' source pages")
    rs = sub.add_parser("research", help="fresh public information").add_subparsers(dest="sub", required=True)
    add(rs, "page", cmd_research_page, "fetch a public page (static HTML) with a timestamp").add_argument("url")
    sp = add(rs, "stale", cmd_research_stale, "sources due for a re-check")
    sp.add_argument("--days", type=int, default=180)
    sp.add_argument("--top", type=int, default=20)
    add(sub, "interview-patterns", cmd_interview_patterns, "what interview loops share across companies (steps, order, AI policy)").add_argument("--json", action="store_true")
    pp = sub.add_parser("prep", help="interactive interview preparation check").add_subparsers(dest="sub", required=True)
    sp = add(pp, "check", cmd_prep_check, "walk the readiness checklist for a company")
    sp.add_argument("company")
    sp.add_argument("--level", type=int, default=3)
    sp.add_argument("--yes", help="comma list of item ids you have done")
    sp.add_argument("--answers", help="JSON file {item_id: true|false}")
    sp.add_argument("--json", action="store_true")
    sp.add_argument("--top", type=int, default=8)
    sp = add(pp, "story", cmd_prep_story, "add a STAR story to your bank")
    sp.add_argument("title")
    sp.add_argument("--competencies", default="")
    sp.add_argument("--situation")
    sp.add_argument("--action")
    sp.add_argument("--result")
    ml = sub.add_parser("mail", help="read-only mail metadata: Outlook, Fastmail, Proton").add_subparsers(dest="sub", required=True)
    add(ml, "scan", cmd_mail_scan, "find LinkedIn job alerts and interview emails").add_argument("--days", type=int, default=7)
    sc = sub.add_parser("secret", help="store a secret in the macOS Keychain").add_subparsers(dest="sub", required=True)
    add(sc, "set", cmd_secret_set, "hidden prompt, then Keychain").add_argument("service")
    con = sub.add_parser("connectors", help="configured connectors").add_subparsers(dest="sub", required=True)
    add(con, "list", cmd_connectors_list, "list connectors and the policy")

    mc = sub.add_parser("machine", help="corporate or personal machine role").add_subparsers(dest="sub", required=True)
    add(mc, "show", cmd_machine_show, "show the role and plugin hints")
    sp = add(mc, "new-user", cmd_machine_new_user, "set yourself up on a corporate machine with the company plugin: role, profile, your repos and connectors, readiness")
    sp.add_argument("--plugin", help="install the company plugin first (git URL or folder)")
    sp.add_argument("--repos", default="~/apps", help="folder with your work repos (default ~/apps)")
    sp.add_argument("--email", help="your work email (default: git config user.email)")
    sp.add_argument("--name", help="your name (default: git config user.name)")
    sp.add_argument("--employer", help="the company name (default: the plugin's first sensitive term)")
    sp.add_argument("--github-user", dest="github_user", help="your login on the company GitHub")
    sp = add(mc, "split-plugin-evidence", cmd_machine_split_plugin_evidence, "move per-person sources and connectors from the company plugin's evidence file into your private files, as they are; shared ones stay")
    sp.add_argument("--dry-run", dest="dry_run", action="store_true", help="show the counts it would reach and write nothing")
    add(mc, "ready", cmd_machine_ready, "check a corporate machine is ready (role, plugin, ladder, terms, no leaks)")
    sp = add(mc, "set", cmd_machine_set, "set the role")
    sp.add_argument("role", choices=machine.ROLES)
    sp.add_argument("--label")
    pr = sub.add_parser("pair", help="pair a corporate and a personal machine").add_subparsers(dest="sub", required=True)
    add(pr, "init", cmd_pair_init, "create a pairing key")
    add(pr, "join", cmd_pair_join, "join with a pairing key").add_argument("key", nargs="?")
    sy = sub.add_parser("sync", help="encrypted bundles between paired machines").add_subparsers(dest="sub", required=True)
    add(sy, "export", cmd_sync_export, "write an encrypted bundle (policy-filtered)").add_argument("file")
    add(sy, "import", cmd_sync_import, "import a bundle from the other machine").add_argument("file")

    dy = sub.add_parser("day", help="your day: watcher, results, plan, next step").add_subparsers(dest="sub", required=True)
    sp = add(dy, "log", cmd_day_log, "today's log: done today, plan for tomorrow, blockers, questions (kept per day)")
    sp.add_argument("--date")
    sp.add_argument("--show", action="store_true")
    sp.add_argument("--list", action="store_true", help="list the kept logs")
    add(dy, "inbox", cmd_day_inbox, "proposals and questions careerbrew has for you: what, why, from, links, and what Accept and Dismiss do").add_argument("--group", help="list each suggestion of one group (Review each in the web app)")
    for name, verb in (("accept-all", "accept"), ("dismiss-all", "dismiss")):
        sp = add(dy, name, cmd_day_group, f"{verb} every suggestion of one group of alike suggestions at once (needs --yes)")
        sp.add_argument("group", help="the group id from `careerbrew day inbox` (grp-...)")
        sp.add_argument("--yes", action="store_true", help=f"really {verb} them all")
        sp.add_argument("--count", type=int, help="the number of suggestions you were shown; nothing changes if the group now holds another number")
    add(dy, "reopen", cmd_day_decide, "bring back a suggestion careerbrew hid as done elsewhere").add_argument("id")
    sp = add(dy, "send", cmd_day_send, "send today's PM update: Slack (default, needs --yes) or an email draft")
    sp.add_argument("--yes", action="store_true", help="really post it (Slack)")
    sp.add_argument("--via", choices=("slack", "email"), help="override pm_send_via once")
    sp.add_argument("--text", help="send this text instead of the generated update")
    sp = add(dy, "pm-sent", cmd_day_pm_sent, "record the PM update you sent (a text file) so the next draft learns from it")
    sp.add_argument("file", help="a text file with the update as you sent it")
    sp.add_argument("--day", help="the day it was for (YYYY-MM-DD; default today)")
    sp = add(dy, "pm-grade", cmd_day_pm_grade, "grade today's PM update against your next ladder level (for you only)")
    sp.add_argument("--record", action="store_true", help="keep the tagged lines as evidence for work promo")
    sp.add_argument("--json", action="store_true")
    add(dy, "accept", cmd_day_decide, "apply a proposal (goal proposals: web app only)").add_argument("id")
    add(dy, "dismiss", cmd_day_decide, "drop a proposal or question").add_argument("id")
    sp = add(dy, "answer", cmd_day_decide, "answer a question; careerbrew applies it")
    sp.add_argument("id")
    sp.add_argument("text")
    sp = add(dy, "suggest", cmd_day_suggest, "a plugin adds its own suggestion to the inbox: what, why, and what Accept does")
    sp.add_argument("--plugin", required=True, help="the installed plugin that suggests it")
    sp.add_argument("--what", required=True, help="what the suggestion is, in one line")
    sp.add_argument("--why", required=True, help="the evidence: why the plugin suggests it")
    sp.add_argument("--accept", required=True, help="what accepting it does, in the owner's words")
    sp.add_argument("--ref", help="a stable id from the feed, so it never shows twice (default: the --what text)")
    sp.add_argument("--task", help="on accept, add this task to Week (leave out to only record the decision)")
    sp.add_argument("--link", help="a link to what prompted it (kept on the task)")
    add(dy, "suggestions", cmd_day_suggestions, "plugin suggestions and your decision on each (ids and refs only)").add_argument("--plugin")
    sp = add(dy, "watch", cmd_day_watch, "toggle the background day watcher")
    sp.add_argument("action", choices=["on", "off", "pause", "resume", "status", "once", "reload"])
    sp.add_argument("--interval", type=int, help="seconds between cycles (default 300, adaptive)")
    sp = add(dy, "results", cmd_day_results, "the results-of-the-day list")
    sp.add_argument("--date")
    sp.add_argument("--show", action="store_true")
    add(dy, "plan", cmd_day_plan, "plan today from calendar facts, tasks, and calibration").add_argument("--date")
    add(dy, "next", cmd_day_next, "what to work on right now, and why")
    sp = add(dy, "handoff", cmd_day_handoff, "the next-day handoff: show yesterday's for this morning, or add and drop today's items (issue #210)")
    sp.add_argument("action", choices=["show", "add", "done"])
    sp.add_argument("section", nargs="?", help="add: calendar, prs, waiting, questions, promises, unverified, lessons, or gotchas; done: the item id")
    sp.add_argument("text", nargs="?", help="add: the item, short (ids and links, no message bodies)")
    sp.add_argument("--link", default="", help="add: a link to the thing")
    sp.add_argument("--today", action="store_true", help="show: today's handoff so far instead of the one for this morning")
    sp.add_argument("--json", action="store_true", help="show: ids, sections, and links for an agent (text only when business data may show)")
    add(dy, "run", cmd_day_run, "the work day plan for /work: steps from careerbrew, the work_steps setting, and plugin manifests, with today's status rows").add_argument(
        "--json", action="store_true", help="the plan for an agent (row text only when business data may show)"
    )
    sp = add(dy, "update", cmd_day_update, "write one /work status row: a step's status, a one-line summary, what it needs from you, and a link")
    sp.add_argument("step", help="a step of today's plan, for example /careerbrew (see `careerbrew day run`)")
    sp.add_argument("--status", choices=["waiting", "running", "done", "blocked", "failed"])
    sp.add_argument("--summary", help="one line: what the step did or found (at most 200 characters)")
    sp.add_argument("--needs-you", dest="needs_you", help="what the step needs from you, one line (empty clears it)")
    sp.add_argument("--link", help="a PR, report, or page for the step (a full https:// URL; empty clears it)")
    sp.add_argument("--handled", action="append", default=[], help="a checkout or PR the step handled today, so later steps skip it: checkout:<path> or pr:<repo>#<number> (repeat it)")
    sp.add_argument("--error-class", dest="error_class", choices=_error_classes(), help="with --status failed: what broke (a blocked row passes --needs-you instead)")
    add(dy, "check", cmd_day_check, "the next /work pass: a running step past its timeout goes back for one retry, then is marked failed").add_argument(
        "--json", action="store_true", help="the steps it set back for a retry or marked failed, for an agent"
    )
    sp = add(dy, "start", cmd_day_start, "start the /work day once, before the first step: fast-forward every checkout the steps read (never a reset), check the shell, and probe sign-in")
    sp.add_argument("--json", action="store_true", help="each checkout's refresh, the shell checks, and the sign-in result, for an agent")
    sp.add_argument("--update-only", action="store_true", help="first refresh source checkouts, dependencies, launcher, plugins and configured agent commands; read no old plan")

    sp.add_argument("--fix", action="store_true", help="repair only an empty launchd GITHUB_TOKEN connector in Claude settings; requires explicit owner approval with --yes; does not start the day")
    sp.add_argument("--yes", action="store_true", help="the owner approved changing Claude connector settings for --fix")
    sp = add(dy, "history", cmd_day_history, "how the /work day went across days: each step's runs, done, blocked, failed, and median time, the step that fails most first")
    sp.add_argument("--days", type=_history_days, default=7, metavar="N", help="how many days back, today included (1-120, default 7)")
    sp.add_argument("--json", action="store_true", help="the steps and days, for an agent (a plugin's step names hidden unless business data may show)")
    add(dy, "render", cmd_day_render, "write the /work page for today: done, blocked, and what is left, one row per step")
    add(dy, "end", cmd_day_end, "end the /work day: read careerbrew's PM draft (never sent), write tomorrow's handoff (open PRs and questions, each with its recheck), stop careerbrew's loops, render the page").add_argument(
        "--json", action="store_true", help="counts, ids, and the page path for an agent (never the draft text)"
    )
    sp = add(dy, "wrap", cmd_day_wrap, "show, move or hold today's wrap-up; an explicit day end still ends now")
    wrap = sp.add_mutually_exclusive_group()
    wrap.add_argument("--at", help="move today's wrap-up to local HH:MM; wrap-up and clock end must both stay before midnight; clears hold")
    wrap.add_argument("--hold", action="store_true", help="hold automatic wrap-up and clock ending until --at resumes it")
    sp.add_argument("--json", action="store_true", help="today's wrap-up time, hold and phase, without business text")
    dq = dy.add_parser("question", help="the work day's question records: a step saves each ask (never asks mid-run); recheck drops closed ones before the one batched ask").add_subparsers(dest="action")
    sp = dq.add_parser("add", help="save a step's question for the batched ask: the ask, its options, your pick, and a recheck that says whether it is still open")
    sp.add_argument("step", help="the step that raises it, for example /careerbrew (see `careerbrew day run`)")
    sp.add_argument("text", help="the ask, one line (at most 200 characters; put any follow-up goal text here)")
    sp.add_argument("--option", action="append", default=[], help="one answer the user can pick (repeat it, at most 6)")
    sp.add_argument("--pick", help="your pick: one of the --option values")
    sp.add_argument("--recheck", help="a command (exit 0 means still open; run without a shell, its output never read) or an http(s):// link an agent checks")
    sp.add_argument("--plugin", help="the installed plugin whose manifest names the recheck (its command runs only with --allow-exec)")
    sp = dq.add_parser("list", help="open questions of today and earlier days, each with its step and age (text only when business data may show)")
    sp.add_argument("--json", action="store_true")
    sp = dq.add_parser("recheck", help="the batched ask: run every recheck, drop the closed items, and list the rest with their age")
    sp.add_argument("--json", action="store_true")
    dq.add_parser("close", help="close a question the user answered, or one an agent found done").add_argument("id")
    dy.choices["question"].set_defaults(fn=cmd_day_question, story_cmd="question", action="list", json=False)
    for sp in dq.choices.values():
        sp.set_defaults(fn=cmd_day_question, story_cmd="question")
    sp = add(dy, "loop", cmd_day_loop, "the one work-day watch loop: a pass runs the live feed and each step's cycle under a lock, every 1.2 x the median pass; end stops every recorded loop")
    sp.epilog = "To pause automatic day end while watching, use careerbrew day wrap --hold."
    sp.add_argument("action", choices=["pass", "run", "status", "add", "stop", "end", "catch-up"])
    sp.add_argument("--kind", choices=["cron", "monitor", "agent", "session", "day-watcher"], help="add: what kind of loop you started")
    sp.add_argument("--id", help="add, stop: the loop's id in its own tool")
    sp.add_argument("--step", help="add: the work step the loop serves, for example /checkup")
    sp.add_argument("--json", action="store_true", help="pass, status, end: ids, kinds, counts, and times for an agent")

    nt = sub.add_parser("notify", help="notifications about important new events: pages, DMs, review requests, mentions, suggestions (opt-in)").add_subparsers(dest="sub", required=True)
    add(nt, "status", cmd_notify, "on or off, the rules (priority, quiet hours, daily cap), and how pop-ups show here (alerter, terminal-notifier, osascript)")
    sp = add(nt, "check", cmd_notify, "check for important new events now and pop up at most one notification (the watcher does this every cycle)")
    sp.add_argument("--wait", action="store_true", help="with alerter: wait for your click on the pop-up and apply it")
    sp = add(nt, "list", cmd_notify, "open notifications: what happened, why it matters, the next step (titles only with --show, to your terminal)")
    sp.add_argument("--all", action="store_true", help="also the closed ones (done, dismissed)")
    sp.add_argument("--show", action="store_true")
    sp = add(nt, "act", cmd_notify, "act on a notification: read, accept or dismiss a suggestion (decides it in the inbox), dismiss, or snooze")
    sp.add_argument("id")
    sp.add_argument("action", choices=["read", "accept", "dismiss", "snooze"])
    add(nt, "open", cmd_notify, "open a notification in the web app (marks it read; never prints the token); a pop-up click runs this").add_argument("id", nargs="?")

    add(sub, "review", cmd_review, "measure outcomes; KEEP / WATCH / PIVOT").add_argument("--html", action="store_true")

    dr = sub.add_parser("draft", help="reply drafts that fit the thread's newest state, saved safely (issue #269)").add_subparsers(dest="sub", required=True)
    sp = add(dr, "fit", cmd_draft_fit, "what a reply should be on the thread's newest message: none, emoji, ack, stop, or reply (prints no thread text)")
    sp.add_argument("--thread", required=True, help='a JSON file: {"messages": [{"ts": ..., "text": ..., "mine": false}, ...]}')
    sp.add_argument("--draft", help="a text file with the reply you drafted")
    sp.add_argument("--seen", help="the ts of the newest message your draft answers (newer ones make it stale)")
    sp.add_argument("--major", help="after a release started: the major issue that must stop it, in one line")
    sp.add_argument("--json", action="store_true")
    sp = add(dr, "replace", cmd_draft_replace, "save a reply draft safely: clear the old one, save, read it back, fail on a mismatch (one writer per target)")
    sp.add_argument("target", help="the draft's target, for example C123/1700000000.000100 (a channel and thread)")
    sp.add_argument("--file", required=True, help="a text file with the draft")
    sp.add_argument("--session", help="the project session that writes it: claims the target first; exit 1 when another session claimed it (issue #274)")
    sp.add_argument("--json", action="store_true")
    sp = add(dr, "show", cmd_draft_show, "read one saved draft (a live cycle's reply draft or review notes); the text shows only in your terminal or the web app")
    sp.add_argument("target", help="the draft's target, as the live cycle prints it (chat:a1, review:<digest>)")
    sp = add(dr, "check", cmd_draft_check, "compare a draft you saved in a chat tool with what it reads back; exit 1 when they differ (prints no text)")
    sp.add_argument("--sent", required=True, help="a text file with the draft you saved")
    sp.add_argument("--read-back", dest="read_back", required=True, help="a text file with what the chat tool shows now")
    sp = add(dr, "route", cmd_draft_route, "where an access ask goes: the owning team's channel, and an open thread to link instead of a new one (prints ids only)")
    sp.add_argument("--ask", required=True, help="a text file with the blocked message")
    sp.add_argument("--directory", required=True, help="a JSON file of support channels (id, owns, members) and open threads")
    sp.add_argument("--teammate", help="the blocked teammate's member id, to tag in the thread or link from the original")
    sp.add_argument("--json", action="store_true")
    sp = add(dr, "judge", cmd_draft_judge, "check a draft's channel before it is saved: the owning channel, no duplicate thread, every mention a member; exit 1 on a failure")
    sp.add_argument("--draft", required=True, help="a text file with the draft")
    sp.add_argument("--channel", required=True, help="the channel id the draft goes to")
    sp.add_argument("--directory", required=True, help="the same JSON file `draft route` reads")
    sp.add_argument("--new-thread", dest="new_thread", action="store_true", help="the draft opens a new thread (fails when an open thread asks the same)")
    return p


# Gate steps that validate tracked repo content and read no data root: they run on any machine role (issue #26).
REPO_CHECKS = {"cards.public-check"}


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args, unknown = parser.parse_known_args(argv)
    if unknown:
        if args.cmd == "data" and getattr(args, "sub", "") == "set":
            parser.error("invalid data set arguments")
        parser.error("unrecognized arguments: " + " ".join(unknown))
    name = f"{args.cmd}.{getattr(args, 'sub', '') or ''}".rstrip(".")
    started = time.monotonic()
    try:
        if name not in REPO_CHECKS:
            machine.guard(args.cmd)
        rc = args.fn(args)
    except (machine.RoleError, sync.SyncError, privacy.PrivacyError, plugins.PluginError, promo.LadderError, KeyError, ValueError, PermissionError, FileNotFoundError, RuntimeError) as exc:
        events.emit("cli.error", command=name, error=type(exc).__name__)
        say(f"error: {exc}")
        return 2
    from . import time_saved

    events.emit("cli.run", command=name, status="ok" if rc == 0 else "fail", ms=int((time.monotonic() - started) * 1000), saves=time_saved.COMMAND_ACTION.get(name))
    return rc


if __name__ == "__main__":
    sys.exit(main())
