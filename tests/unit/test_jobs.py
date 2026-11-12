from careerbrew.jobs import criteria, sources, strategy, want
from careerbrew.tutor import engine as tutor


def test_scan_normalizes_three_ats_formats(ats_sources):
    ps = sources.scan()
    by_id = {p.id: p for p in ps}
    assert len(ps) == 8
    gh = by_id["gh:Northwind:101"]
    assert gh.seniority == 4 and gh.remote and (gh.salary_min, gh.salary_max) == (210000, 260000)
    assert {"go", "kubernetes", "kafka", "distributed systems", "system design"} <= set(gh.skills)
    lever = by_id["lever:Contoso:c-1"]
    assert lever.salary_max == 200000 and "terraform" in lever.skills
    ashby = by_id["ashby:Fabrikam:f-1"]
    assert ashby.remote and ashby.salary_min == 190000 and "llm" in ashby.skills


def test_scan_caches_each_source(ats_sources, home):
    sources.scan()
    assert len(list((home / "cache").glob("*.json"))) == 3


def test_criteria_rejects_with_reasons(ats_sources):
    kept, rejected = criteria.apply(sources.scan())
    titles = {p.title for p in kept}
    assert "Engineering Manager" not in titles and "Software Engineering Intern" not in titles
    assert rejected["title-excluded"] == 2
    assert "Senior Data Engineer" not in titles  # Sample Town, not remote
    assert rejected["location"] == 1


def test_want_score_explains_and_ranks(ats_sources):
    ps = sources.scan()
    prof, crit = want.profile(), criteria.load()
    scores = {p.id: want.score(p, want.weights(), prof, crit) for p in ps}
    s_ai, f_ai = scores["ashby:Fabrikam:f-1"]
    s_ios, _ = scores["ashby:Fabrikam:f-2"]
    assert 0 <= s_ios < s_ai <= 100
    assert set(f_ai) == set(want.DEFAULT_WEIGHTS)


def test_learn_weights_needs_five_ratings_then_keeps_or_reverts(ats_sources):
    ps = sources.scan()
    prof, crit = want.profile(), criteria.load()
    want.rate(ps[0].id, 3)
    assert want.learn_weights(ps, prof, crit)["status"] == "need-more-ratings"
    ratings = {"ashby:Fabrikam:f-1": 5, "gh:Northwind:101": 5, "lever:Contoso:c-1": 3, "gh:Northwind:102": 2, "ashby:Fabrikam:f-2": 1, "lever:Contoso:c-2": 2}
    for pid, r in ratings.items():
        want.rate(pid, r)
    res = want.learn_weights(ps, prof, crit)
    assert res["status"] in {"kept", "reverted"}
    assert res["after"] is not None
    if res["status"] == "kept":
        assert res["before"] is None or res["after"] >= res["before"]


def test_spearman():
    assert want.spearman([1, 2, 3, 4], [10, 20, 30, 40]) == 1.0
    assert want.spearman([1, 2, 3, 4], [4, 3, 2, 1]) == -1.0
    assert want.spearman([1, 2], [1, 2]) is None


def test_strategy_tiers_one_company_once_and_gates(ats_sources):
    sources.scan()
    want.rate("gh:Northwind:101", 5)
    plan = strategy.build()
    assert not any(plan["tiers"].values()), "careerbrew never fills a tier on its own"
    assert plan["steps"][0].startswith("First: choose the jobs you want")
    suggested = [r["company"] for rows in plan["suggested"].values() for r in rows]
    assert len(suggested) == len(set(suggested)), "a company is suggested in exactly one tier"
    strategy.choose("gh:Northwind:101", "dream")
    plan = strategy.build()
    assert [r["company"] for r in plan["tiers"]["dream"]] == ["Northwind"] and plan["tiers"]["dream"][0]["chosen"]
    assert plan["phase"] == "learn"  # no drills yet, readiness 0
    assert plan["steps"][0].startswith("NOW")
    assert plan["skill_gaps"], "dream/target skills the profile lacks become gaps"
    assert [i for i in tutor.items() if i.startswith("gap-")]


def test_strategy_moves_to_target_after_practice_passes(ats_sources, monkeypatch):
    sources.scan()
    monkeypatch.setattr(strategy.tutor, "readiness", lambda topics, at=None: 0.75)
    for c in ("P1", "P2", "P3"):
        strategy.log_application(c, "practice", "applied")
        strategy.log_application(c, "practice", "screen")
    strategy.log_application("P1", "practice", "onsite")
    strategy.log_application("P2", "practice", "onsite")
    assert strategy.pass_rates()["practice"] == {"loops": 3, "pass_rate": 0.667}
    assert strategy.build()["phase"] == "target"


def test_only_the_owner_chooses_jobs_and_can_take_them_back(ats_sources):
    import pytest

    sources.scan()
    with pytest.raises(ValueError, match="tier"):
        strategy.choose("gh:Northwind:101", "maybe")
    with pytest.raises(KeyError):
        strategy.choose("gh:Nope:1", "dream")
    strategy.choose("gh:Northwind:101", "target")
    assert strategy.chosen() == {"gh:Northwind:101": "target"}
    assert strategy.unchoose("gh:Northwind:101") == {}
