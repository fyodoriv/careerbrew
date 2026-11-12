from pathlib import Path

import pytest
import yaml

from careerbrew import store
from careerbrew.tutor import cards, public

from .conftest import CANARIES

EXPLANATION = "\n\n".join(f"{s}\n" + " ".join(["word"] * 40) for s in public.SECTIONS)


def card(q="What is a heap?", **kw):
    return {"source_id": public.qhash(q), "question": q, "answer": "A tree where each parent is ordered before its children.", "topic": "computer science", "tags": ["heap"], "explanation": EXPLANATION, **kw}


@pytest.fixture
def deck(tmp_path, monkeypatch):
    monkeypatch.setattr(public, "cards_dir", lambda: tmp_path / "cards")
    return tmp_path / "cards"


@pytest.mark.skipif(not yaml.__with_libyaml__, reason="PyYAML built without libyaml")
def test_the_committed_deck_parses_with_the_c_loader_to_the_same_cards(monkeypatch):
    # The pure-Python parser read the 2 MB deck in 1.3 s at load ~70 and 5.8 s at load ~98:
    # the first Practice load in a new server missed Playwright's 5 s wait (us-35).
    files = tuple((str(f), f.stat().st_mtime_ns) for f in sorted(public.REPO_CARDS.glob("*.yaml")))
    expected = [c for f, _ in files for c in (yaml.safe_load(Path(f).read_text(encoding="utf-8")) or {}).get("cards", [])]
    assert len(expected) > 100
    public._load.cache_clear()

    def pure_python(*_a, **_k):
        raise AssertionError("the deck went through the pure-Python YAML parser")

    monkeypatch.setattr(yaml, "safe_load", pure_python)
    try:
        assert list(public._load(files)) == expected
    finally:
        public._load.cache_clear()


def test_publish_merges_a_card_that_meets_the_bar(deck):
    assert public.publish([card()]) == {"published": 1, "skipped": 0, "total": 1, "private": 0}
    [c] = public.load()
    assert c["id"].startswith("kb-") and c["prompt"] == public.prompt_version() and (deck / "computer-science.yaml").exists()
    assert public.check() == []


def test_publish_blocks_cards_below_the_bar_and_writes_nothing(deck):
    short = card(explanation="## In plain words\ntoo short")
    with pytest.raises(public.PublishError, match="explanation has .* words"):
        public.publish([card("What is a stack?"), short])
    assert public.load() == [], "one bad card blocks the whole publish"


def test_publish_blocks_personal_data(deck, home):
    store.write_yaml(home / "profile.yaml", {"name": CANARIES[0], "employer": CANARIES[1]})
    leaky = card(explanation=EXPLANATION + f"\nAt {CANARIES[1]} we did this.")
    with pytest.raises(public.PublishError, match="personal data") as e:
        public.publish([leaky])
    assert CANARIES[1] not in str(e.value), "the error names a term id, never the term"


def test_owner_cards_get_the_explanation_and_hide_the_public_duplicate(deck):
    cards.write_deck("notion", [{"id": "n1", "question": "what is a HEAP", "answer": "my words", "tags": ["Computer Science"]}])
    public.publish([card(source_id=public.qhash("what is a HEAP"))])
    all_ = cards.all_cards()
    assert set(all_) == {"n1"}, "the owner reviews their own card, not a second copy"
    [q] = cards.queue()
    assert q["id"] == "n1" and "## In plain words" in q["explanation"]


def test_public_deck_is_studyable_without_any_private_cards(deck):
    public.publish([card()])
    [q] = cards.queue()
    assert q["deck"] == "public" and q["explanation"]


def test_queue_lists_new_cards_and_keeps_behavioural_answers_private(deck):
    cards.write_deck(
        "notion",
        [
            {"id": "a", "question": "What is a trie?", "answer": "prefix tree", "tags": ["Computer Science"]},
            {"id": "b", "question": "Tell me about a conflict", "answer": f"my story with {CANARIES[0]}", "tags": ["Behavioural"]},
        ],
    )
    items = {i["question"]: i for i in public.explain_queue()}
    assert items["What is a trie?"]["hint"] == "prefix tree"
    assert items["Tell me about a conflict"]["hint"] == "", "the owner's story never enters the queue"


def test_personal_questions_get_a_private_explanation_never_a_public_one(deck, home):
    q = "What did I ship at my last job?"
    cards.write_deck("notion", [{"id": "a", "question": q, "answer": "x", "tags": ["Behavioural"]}])
    assert public.publish([{"source_id": public.qhash(q), "skip": True, "reason": "personal"}])["skipped"] == 1
    [item] = public.explain_queue()
    assert item["private"] and item["reason"].startswith("personal")
    r = public.publish([{**card(q), "source_id": public.qhash(q), "private": True}])
    assert r["private"] == 1 and public.load() == [], "nothing personal reaches knowledge/cards/"
    assert public.explain_queue() == []
    [qc] = cards.queue()
    assert qc["explanation"].startswith("## In plain words")
    raw = (home / "tutor" / "private-explanations.yaml").read_bytes()
    assert raw.startswith(b"CBV1"), "private explanations are encrypted at rest"
    assert [p["section"] for p in public.library_entries()] == ["my-explanations"]


def test_a_better_prompt_or_failing_learners_send_cards_back_for_a_rewrite(deck, monkeypatch):
    cards.write_deck("notion", [{"id": "n1", "question": "What is a heap?", "answer": "x", "tags": ["Computer Science"]}])
    public.publish([card()])
    assert public.explain_queue() == []
    current = public.prompt_version
    monkeypatch.setattr(public, "prompt_version", lambda: "newer000")
    [item] = public.explain_queue()
    assert item["reason"] == "written with an older prompt" and item["rewrite"].startswith("kb-")
    monkeypatch.setattr(public, "prompt_version", current)
    for _ in range(public.LAPSES_TO_REWRITE):
        cards.review("n1", "again")
    [item] = public.explain_queue()
    assert "clearer" in item["reason"]


def test_library_has_one_page_per_card_grouped_by_topic(deck):
    public.publish([card(), card("What is a stack?")])
    pages = public.library_entries()
    assert [p["title"] for p in pages] == ["What is a heap?", "What is a stack?"]
    assert {p["section"] for p in pages} == {"knowledge:computer science"} and pages[0]["markdown"].startswith("**Short answer.**")


def test_the_committed_public_deck_meets_the_bar(monkeypatch):
    monkeypatch.delenv("CAREERBREW_PUBLIC_CARDS")
    assert public.load(), "the repo ships a public deck"
    assert public.check() == []


def test_duplicate_questions_are_queued_once_and_local_http_is_fine(deck):
    cards.write_deck("notion", [{"id": "a", "question": "What is a trie?", "answer": "x", "tags": ["CS"]}, {"id": "b", "question": "what is a TRIE", "answer": "y", "tags": ["CS"]}])
    assert len(public.explain_queue()) == 1
    ok = card(explanation=EXPLANATION + "\n`proxy_pass http://localhost:8080;`")
    assert public.lint({**ok, "id": "kb-x"}) == []
    assert public.lint({**card(explanation=EXPLANATION + " http://evil.test/x"), "id": "kb-y"})
    xml = card(explanation=EXPLANATION + '\n`<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">`')
    assert public.lint({**xml, "id": "kb-z"}) == [], "XML namespace names are identifiers, not links"


def test_the_personal_check_is_as_strict_as_the_repo_leak_scan(deck, home):
    store.write_text(home / "sensitive-terms.txt", "Zzcorp\n")
    assert public.personal_hits(card(explanation=EXPLANATION + " At zzcorp we shipped it."))
    assert public.personal_hits(card(explanation=EXPLANATION + " Zzcorporation too.")), "inside words, like the gate"


def test_a_reader_report_sends_the_card_back_and_a_rewrite_clears_it(deck):
    public.publish([card()])
    [pub] = public.load()
    public.flag(pub["id"], "the example is wrong")
    [item] = public.explain_queue()
    assert item["reason"] == "a reader reported: the example is wrong" and item["rewrite"] == pub["id"]
    public.publish([{**card(), "rewrite": pub["id"]}])
    assert public.flags() == {} and public.explain_queue() == []
    with pytest.raises(KeyError):
        public.flag("kb-nope", "x")
