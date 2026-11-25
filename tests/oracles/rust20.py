"""rust-20 Python oracle for tutor cards, public decks, and Notion imports.

Every case uses invocation-owned synthetic data and fixed clocks. Reports contain
counts, identities, and numeric scheduling state, never private answer text.
No production route, owner file, or live account is used.
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import sys
import time
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import fsrs

from careerbrew import machine, notion_import, paths, prep, store
from careerbrew.jobs import strategy
from careerbrew.tutor import cards, engine, mistakes, public
from careerbrew.work import evidence

CASES = (
    "depvec-fsrs",
    "fsrs-old-state",
    "cards-and-decks",
    "local-day-stats",
    "public-deck",
    "notion-idempotence",
    "notion-validation-and-partial-writes",
    "providers-and-mistakes",
)
ROOT = Path(__file__).resolve().parents[2]
FIX = ROOT / "tests" / "fixtures"
BUNDLE = FIX / "rust20-notion-bundle"
T0 = datetime(2026, 10, 19, 9, 0, tzinfo=UTC)
CANARIES = (b"RUST20CANARYPERSON", b"RUST20CANARYEMPLOYER", b"RUST20CANARYANSWER")
EXPLANATION = "\n\n".join(f"{section}\n" + " ".join(["word"] * 40) for section in public.SECTIONS)

# Fixed differential vectors never use random interval fuzzing. Production code is
# unchanged; this replacement has process scope only in the oracle invocation.
cards.Scheduler = lambda: fsrs.Scheduler(enable_fuzzing=False)
engine.Scheduler = lambda: fsrs.Scheduler(enable_fuzzing=False)


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, set):
        return _typed(sorted(value))
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.public = self.base / "public"
        self.root.mkdir(parents=True)
        self.root.chmod(0o700)
        self.public.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
                "CAREERBREW_PUBLIC_CARDS": str(self.public),
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
            }
        )
        time.tzset()


def _card(card_id: str, topic: str, number: int = 0) -> dict[str, Any]:
    return {
        "id": card_id,
        "question": f"{topic} question {number}",
        "answer": f"synthetic technical answer {number}",
        "topic": topic,
        "tags": [topic],
    }


def _public_card(question: str = "What is a heap?", **updates: Any) -> dict[str, Any]:
    row = {
        "source_id": public.qhash(question),
        "question": question,
        "answer": "A tree where each parent is ordered before its children.",
        "topic": "computer science",
        "tags": ["heap"],
        "explanation": EXPLANATION,
    }
    row.update(updates)
    return row


def _depvec_fsrs(_base: Path) -> dict[str, Any]:
    fixture = json.loads((ROOT / "xtask" / "tests" / "fixtures" / "depvec" / "fsrs.json").read_text())
    spec = importlib.util.spec_from_file_location("rust20_depvec_driver", ROOT / "xtask" / "tests" / "fixtures" / "depvec" / "driver.py")
    assert spec and spec.loader
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    results = []
    for vector in fixture["vectors"]:
        try:
            output = {"ok": driver.fsrs_schedule(vector["op"], vector["input"])}
        except Exception:
            output = {"error": True}
        results.append({"id": vector["id"], "output": output})
    return {"set": "fsrs", "results": results, "vector_count": len(results)}


def _fsrs_old_state(_base: Path) -> dict[str, Any]:
    Workspace(_base, "old-state")
    cards.write_deck("test", [_card("c1", "coding")])
    cards.review("c1", "good", at=T0)
    cards.review("c1", "good", at=T0 + timedelta(minutes=10))
    path = paths.sub("tutor", "card-state.json")
    raw = store.read_json(path, {})
    raw["c1"]["card_id"] = 424242
    # Fractional old timestamps exercise lossless datetime read/write as well.
    raw["c1"]["due"] = raw["c1"]["due"].replace("09:10:00+00:00", "09:10:00.123456+00:00")
    store.write_json(path, raw)
    old = cards._states()["c1"].to_dict()
    before = store.read_json(path, {})["c1"]
    row = cards.review("c1", "good", at=T0 + timedelta(days=3))
    new = cards._states()["c1"].to_dict()
    return {"old": old, "old_file": before, "old_roundtrip": old == before, "review": row, "new": new}


def _cards_and_decks(_base: Path) -> dict[str, Any]:
    ws = Workspace(_base, "cards")
    deck = [_card(f"{topic[:2]}{number}", topic, number) for topic in ("coding", "behavioral", "system design") for number in range(2)]
    cards.write_deck("test", deck)
    first = cards.queue(at=T0, new_per_day=4)
    reviewed = [cards.review(row["id"], "good", at=T0) for row in first[:2]]
    remaining = cards.queue(at=T0 + timedelta(minutes=1), new_per_day=4)

    learn_root = ws.root / "learn" / "rust20" / "srs"
    with cards.deck_root(learn_root):
        cards.write_deck("concepts", [_card("learn-c1", "learn")])
        learn_review = cards.review("learn-c1", "good", at=T0)
        learn_ids = sorted(cards.all_cards())
    interview_ids = sorted(cards.all_cards())
    isolation = {
        "learn_ids": learn_ids,
        "interview_ids": interview_ids,
        "learn_review": learn_review,
        "learn_state": (learn_root / "card-state.json").is_file(),
        "interview_has_learn_state": "learn-c1" in cards.scheduled(),
        "learn_has_interview_deck": (learn_root / "decks" / "test.yaml").exists(),
    }

    cards.write_deck("leech", [_card("leech-1", "coding")])
    at = T0
    leech_row: dict[str, Any] = {}
    for _ in range(cards.LEECH_LAPSES):
        cards.review("leech-1", "good", at=at)
        at += timedelta(days=30)
        cards.review("leech-1", "good", at=at)
        at += timedelta(days=60)
        leech_row = cards.review("leech-1", "again", at=at)
        at += timedelta(days=1)
    suspended_absent = "leech-1" not in {row["id"] for row in cards.queue(at=at + timedelta(days=30))}
    cards.unsuspend("leech-1")
    unsuspended_present = "leech-1" in {row["id"] for row in cards.queue(at=at + timedelta(days=30))}
    return {
        "first": [{"id": row["id"], "topic": row["topic"], "mode": row["mode"]} for row in first],
        "reviewed": reviewed,
        "remaining": [row["id"] for row in remaining],
        "isolation": isolation,
        "leech": leech_row,
        "suspended_absent": suspended_absent,
        "unsuspended_present": unsuspended_present,
        "unknown_rating": _error(lambda: cards.review("co0", "great", at=T0)),
        "unknown_card": _error(lambda: cards.review("missing", "good", at=T0)),
    }


def _local_day_stats(_base: Path) -> dict[str, Any]:
    Workspace(_base, "local-day")
    os.environ["TZ"] = "Etc/GMT+4"
    time.tzset()
    cards.write_deck("test", [_card(f"c{number}", "coding", number) for number in range(5)])
    evening = datetime(2026, 10, 19, 23, 30, tzinfo=UTC)
    first = cards.queue(at=evening, new_per_day=3)
    for row in first:
        cards.review(row["id"], "good", at=evening)
    later = evening + timedelta(hours=1)
    queue_later = cards.queue(at=later, new_per_day=3)
    stats = cards.stats(at=later)
    next_day = cards.stats(at=later + timedelta(days=1))
    return {
        "first_count": len(first),
        "later_modes": [row["mode"] for row in queue_later],
        "later_new": [row["id"] for row in queue_later if row["mode"] == "new"],
        "reviewed_today": stats["reviewed_today"],
        "streak_days": stats["streak_days"],
        "next_day_streak": next_day["streak_days"],
        "retention_30d": stats["retention_30d"],
        "by_topic": stats["by_topic"],
    }


def _public_deck(_base: Path) -> dict[str, Any]:
    ws = Workspace(_base, "public")
    store.write_yaml(ws.root / "profile.yaml", {"name": "RUST20CANARYPERSON", "employer": "RUST20CANARYEMPLOYER"})
    cards.write_deck(
        "notion",
        [
            {"id": "n1", "question": "What is a heap?", "answer": "private technical answer", "tags": ["Computer Science"]},
            {"id": "n2", "question": "Tell me about a conflict", "answer": "RUST20CANARYANSWER", "tags": ["Behavioural"]},
        ],
    )
    initial = {row["question"]: row for row in public.explain_queue()}
    published = public.publish([_public_card(source_id=public.qhash("what is a HEAP"))])
    owner_cards = sorted(cards.all_cards())
    explanation = cards.queue()[0]["explanation"].startswith("## In plain words")

    personal_question = "Tell me about a conflict"
    skipped = public.publish([{"source_id": public.qhash(personal_question), "skip": True, "reason": "personal"}])
    private_item = next(row for row in public.explain_queue() if row["question"] == personal_question)
    private_result = public.publish([_public_card(personal_question, source_id=public.qhash(personal_question), private=True)])
    private_sealed = paths.sub("tutor", "private-explanations.yaml").read_bytes().startswith(b"CBV1\n")
    public_ids = [row["id"] for row in public.load()]
    [pub] = public.load()
    public.flag(pub["id"], "the example is wrong")
    flagged = next(row for row in public.explain_queue() if row.get("rewrite") == pub["id"])
    public.publish([_public_card(rewrite=pub["id"])])
    flags_cleared = public.flags() == {}

    before = {path.name: path.read_bytes() for path in ws.public.glob("*.yaml")}
    bad = _public_card("What is a stack?", explanation="## In plain words\ntoo short")
    refused = _error(lambda: public.publish([_public_card("What is a trie?"), bad]))
    after = {path.name: path.read_bytes() for path in ws.public.glob("*.yaml")}
    leaky = _public_card("What is a queue?", explanation=EXPLANATION + "\nAt RUST20CANARYEMPLOYER we did this.")
    leak_refusal = _error(lambda: public.publish([leaky]))
    plugin_root = ws.root / "plugins" / "rust20plug"
    store.write_yaml(plugin_root / "careerbrew-plugin.yaml", {"name": "rust20plug", "version": "0.1.0", "sensitive_terms": "sensitive-terms.txt"})
    store.write_text(plugin_root / "sensitive-terms.txt", "RUST20CANARYPLUGIN\n")
    store.write_json(ws.root / "plugins.lock.json", {"plugins": {"rust20plug": {"origin": "plugin:rust20plug"}}})
    plugin_leaky = _public_card("What is a map?", explanation=EXPLANATION + "\nRUST20CANARYPLUGIN")
    plugin_leak_refusal = _error(lambda: public.publish([plugin_leaky]))
    return {
        "initial_hints": {question: row["hint"] for question, row in initial.items()},
        "published": published,
        "owner_cards": owner_cards,
        "explanation": explanation,
        "skipped": skipped,
        "private_item": private_item,
        "private_result": private_result,
        "private_sealed": private_sealed,
        "public_ids": public_ids,
        "flagged": flagged,
        "flags_cleared": flags_cleared,
        "check": public.check(),
        "partial_write": {"error": refused, "unchanged": before == after},
        "leak_refusal": leak_refusal,
        "plugin_leak_refusal": plugin_leak_refusal,
        "personal_hits": public.personal_hits(leaky),
        "lint": {
            "local_http": public.lint(_public_card(explanation=EXPLANATION + "\n`proxy_pass http://localhost:8080;`", id="kb-local")),
            "remote_http": bool(public.lint(_public_card(explanation=EXPLANATION + " http://evil.test/x", id="kb-remote"))),
        },
        "library_sections": [row["section"] for row in public.library_entries()],
    }


def _seed_non_notion_rows() -> None:
    store.write_jsonl(strategy.applications_path(), [{"company": "User company", "tier": "target", "stage": "applied", "at": "2026-01-01T00:00:00+00:00", "origin": "user"}])
    store.write_jsonl(evidence.evidence_path(), [{"id": "user-1", "kind": "note", "title": "User evidence", "at": "2026-01-01T00:00:00+00:00", "origin": "user"}])
    store.write_yaml(prep.stories_path(), {"stories": [{"id": "user-story", "title": "User story", "origin": "user"}]})


def _notion_idempotence(_base: Path) -> dict[str, Any]:
    ws = Workspace(_base, "notion")
    bundle = ws.root / "imports" / "notion"
    shutil.copytree(BUNDLE, bundle)
    store.write_yaml(ws.root / "profile.yaml", {"name": "Synthetic", "skills": {"python": 4}})
    _seed_non_notion_rows()
    first = notion_import.run(bundle)
    cards.review("notion-f1", "good", at=T0)
    before = {
        "apps": len(store.read_jsonl(strategy.applications_path())),
        "evidence": len(store.read_jsonl(evidence.evidence_path())),
        "stories": len(prep.stories()),
    }
    second = notion_import.run(bundle)
    after = {
        "apps": len(store.read_jsonl(strategy.applications_path())),
        "evidence": len(store.read_jsonl(evidence.evidence_path())),
        "stories": len(prep.stories()),
    }
    origins = {
        "apps": sorted(row["origin"] for row in store.read_jsonl(strategy.applications_path())),
        "evidence": sorted(row["origin"] for row in store.read_jsonl(evidence.evidence_path())),
        "stories": sorted(row["origin"] for row in prep.stories()),
    }
    profile = store.read_yaml(ws.root / "profile.yaml", {})
    return {
        "first": first,
        "second": second,
        "before": before,
        "after": after,
        "origins": origins,
        "check": notion_import.check(bundle),
        "reviewed_today": cards.stats(at=T0)["reviewed_today"],
        "scheduled_kept": "notion-f1" in cards.scheduled(),
        "deck_count": len([row for row in cards.all_cards().values() if row["deck"] == "notion"]),
        "skills": profile["skills"],
        "sealed": all(path.read_bytes().startswith(b"CBV1\n") for path in sorted(bundle.glob("*"))),
        "library_count": len(notion_import.library()),
        "plain": [notion_import.plain("**What is a \\^heap?**"), notion_import.plain("__Why__ `useMemo`?")],
    }


def _write_snapshot(root: Path) -> dict[str, bool]:
    return {
        "library": (root / "library" / "notion" / "index.json").exists(),
        "board": (root / "jobs" / "notion-board.json").exists(),
        "applications": (root / "jobs" / "applications.jsonl").exists(),
        "deck": (root / "tutor" / "decks" / "notion.yaml").exists(),
        "profile": (root / "profile.yaml").exists(),
        "stories": (root / "prep" / "stories.yaml").exists(),
        "evidence": (root / "work" / "evidence.jsonl").exists(),
        "terms": (root / "sensitive-terms.txt").exists(),
    }


def _notion_validation_and_partial_writes(_base: Path) -> dict[str, Any]:
    ws = Workspace(_base, "partial")
    bad = ws.base / "bad"
    shutil.copytree(BUNDLE, bad)
    manifest = json.loads((bad / "manifest.json").read_text())
    manifest["counts"].pop("stories")
    (bad / "manifest.json").write_text(json.dumps(manifest))
    missing_kind = _error(lambda: notion_import.run(bad))
    missing_kind_writes = _write_snapshot(ws.root)

    bundle = ws.base / "bundle"
    shutil.copytree(BUNDLE, bundle)
    real_write_deck = cards.write_deck

    def stop_deck(*_args: Any, **_kwargs: Any) -> int:
        raise RuntimeError("synthetic import stop")

    cards.write_deck = stop_deck
    try:
        partial_error = _error(lambda: notion_import.run(bundle))
    finally:
        cards.write_deck = real_write_deck
    partial = _write_snapshot(ws.root)
    partial["bundle_sealed"] = all(path.read_bytes().startswith(b"CBV1\n") for path in sorted(bundle.glob("*")))
    recovered = notion_import.run(bundle)
    recovered_check = notion_import.check(bundle)
    recovered_counts = {
        "apps": len(store.read_jsonl(strategy.applications_path())),
        "evidence": len(store.read_jsonl(evidence.evidence_path())),
        "stories": len(prep.stories()),
    }

    corporate = Workspace(_base, "corporate")
    machine.set_role("corporate")
    role_error = _error(lambda: notion_import.run(BUNDLE))
    role_writes = _write_snapshot(corporate.root)
    return {
        "missing_kind": missing_kind,
        "missing_kind_writes": missing_kind_writes,
        "partial_error": partial_error,
        "partial": partial,
        "recovered": recovered,
        "recovered_check": recovered_check,
        "recovered_counts": recovered_counts,
        "role_error": role_error,
        "role_writes": role_writes,
    }


def _providers_and_mistakes(_base: Path) -> dict[str, Any]:
    Workspace(_base, "providers")
    added = engine.add_gap_items(["Rust"])
    repeated = engine.add_gap_items(["Rust"])
    item = engine.items()["gap-rust-1"]
    rubric = engine.rubrics()["skill"]
    criteria = [row["id"] for row in rubric["criteria"]]
    seeded = engine.cards()
    seeded["gap-rust-1"] = fsrs.Card(card_id=424243, due=T0)
    engine._save_cards(seeded)
    row = engine.record("gap-rust-1", criteria, at=T0)
    old_cards = {key: value.to_dict() for key, value in engine.cards().items()}
    attempts = engine.attempts()
    next_review = engine.next_item(["Rust"], at=T0 + timedelta(days=20))
    mastery = engine.mastery(T0).get("Rust")
    readiness = engine.readiness(["Rust"], T0)

    first = mistakes.record("ex-1", "arrays", "wrong-answer", "synthetic wrong answer")
    second = mistakes.record("ex-1", "arrays", "wrong-answer", "synthetic repeated answer")
    exercise = {
        "id": "ex-2",
        "pattern": "trees",
        "pitfalls": {},
        "tests": [{"note": "empty input"}],
    }
    outcome = {"results": [{"ok": False, "i": 0}]}
    found = mistakes.from_exercise(exercise, outcome, {})
    summary = mistakes.summary()
    return {
        "added": added,
        "repeated": repeated,
        "item": item,
        "criteria": criteria,
        "record": row,
        "cards": old_cards,
        "attempt_count": len(attempts),
        "next_review": next_review,
        "mastery": mastery,
        "readiness": readiness,
        "now": engine.now().isoformat(timespec="seconds"),
        "rating_thresholds": [engine.score_to_rating(score).name for score in (0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0)],
        "mistakes": {
            "first_repeat": first["repeat"],
            "second_repeat": second["repeat"],
            "from_exercise": [row["kind"] for row in found],
            "deck_count": len([row for row in cards.all_cards().values() if row["deck"] == "mistakes"]),
            "summary": summary,
        },
    }


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust20-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "depvec-fsrs": _depvec_fsrs,
        "fsrs-old-state": _fsrs_old_state,
        "cards-and-decks": _cards_and_decks,
        "local-day-stats": _local_day_stats,
        "public-deck": _public_deck,
        "notion-idempotence": _notion_idempotence,
        "notion-validation-and-partial-writes": _notion_validation_and_partial_writes,
        "providers-and-mistakes": _providers_and_mistakes,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":")) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
