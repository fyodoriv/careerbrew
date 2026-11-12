"""Ladder keyword matching, over-broad warnings, and per-level text (issues #5, #6, #9)."""

from careerbrew.cli import main
from careerbrew.work import evidence, promo


def _comp(keywords, tags=()):
    return {"signals": {"keywords": list(keywords), "tags": list(tags)}}


def test_short_keyword_does_not_match_inside_a_word():
    assert not promo._matches({"title": "fix: fail fast on decision"}, _comp(["ai", "ci"]))
    assert promo._matches({"title": "feat: AI ranking"}, _comp(["ai"]))


def test_star_keyword_matches_a_prefix():
    assert promo._matches({"title": "Lead the database migration"}, _comp(["migrat*"]))
    assert not promo._matches({"title": "Lead the database migration"}, _comp(["migrat"]))


def test_over_broad_competency_warns(home, capsys):
    for i in range(10):
        evidence.note(f"fix: bug {i}", [])
    evidence.note("Mentored a new hire", ["leadership"])
    asm = promo.assess("generic-swe", "senior")
    assert any("quality" in w for w in asm["warnings"])
    assert not any("leadership" in w for w in asm["warnings"])
    assert main(["work", "promo", "--level", "senior"]) == 0
    assert "warning:" in capsys.readouterr().out


def test_level_descriptions_and_examples_render(home, tmp_path):
    lad = tmp_path / "l.yaml"
    lad.write_text(
        "id: t\nlevels: [senior]\ncompetencies:\n"
        "  - {id: d, name: Delivery, weight: 1, signals: {tags: [delivery]}, expect: {senior: 1},\n"
        "     descriptions: {senior: Ships multi-team projects},\n"
        "     examples: {senior: {looks_like: [Led a launch across teams], doesnt_look_like: [Only fixed own bugs]}}}\n"
    )
    promo.import_ladder(lad)
    doc = promo.write_doc(promo.assess("t", "senior")).read_text()
    assert "Ships multi-team projects" in doc and "Led a launch across teams" in doc and "Only fixed own bugs" in doc


def test_bad_descriptions_shape_is_rejected():
    import pytest

    with pytest.raises(promo.LadderError):
        promo.validate_ladder({"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {"a": 1}, "descriptions": "text"}]})


def test_keyword_with_symbols_matches():
    assert promo._matches({"title": "Ported the parser to C++"}, _comp(["c++"]))
    assert promo._matches({"title": "Moved the API to .NET 8"}, _comp([".net"]))
