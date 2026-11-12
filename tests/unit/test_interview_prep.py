import pytest

from careerbrew import interview, prep


def test_every_seeded_company_loop_uses_known_steps_with_tips():
    steps = interview.steps()
    cs = interview.companies()
    assert {"google", "amazon", "stripe", "gitlab", "datadog"} <= set(cs)
    for c in cs.values():
        for s in c["loop"]:
            assert s["step"] in steps
            assert len(steps[s["step"]]["tips"]) >= 3 and len(steps[s["step"]]["traps"]) >= 3


def test_find_company_by_name_or_id_case_insensitive():
    assert interview.find_company("DATADOG")["name"] == "Datadog"
    assert interview.find_company("GitLab")["id"] == "gitlab"
    assert interview.find_company("nobody") is None


def test_default_loop_depends_on_level():
    senior = interview.loop_for("Nobody Inc", 3)
    junior = interview.loop_for("Nobody Inc", 1)
    assert not senior["known"] and "system-design" in [s["id"] for s in senior["loop"]]
    assert "system-design" not in [s["id"] for s in junior["loop"]]


def test_render_numbers_sources_once():
    text = interview.render(interview.loop_for("Stripe"))
    assert "## Sources" in text and text.count("https://") == text.split("## Sources")[1].count("https://")


def test_prep_measures_readiness_and_stories_and_tracks_trend(home):
    first = prep.run("Amazon", answers={})
    gap_ids = {g["id"] for g in first["gaps"]}
    assert "behavioral:stories" in gap_ids and "coding-onsite:readiness" in gap_ids
    assert first["gaps"][0]["weight"] == max(g["weight"] for g in first["gaps"])
    for i in range(6):
        prep.add_story(f"S{i}", ["leadership", "conflict", "impact", "ownership"])
    asks = [i["id"] for i in prep.checklist("Amazon")["items"] if not i["auto"]]
    second = prep.run("Amazon", answers=dict.fromkeys(asks, True))
    assert "behavioral:stories" not in {g["id"] for g in second["gaps"]}
    assert second["score"] > first["score"] and second["trend"] == [first["score"], second["score"]]


def test_prep_interactive_ask_defaults_to_last_answer(home):
    asks = [i["id"] for i in prep.checklist("Stripe")["items"] if not i["auto"]]
    prep.run("Stripe", answers=dict.fromkeys(asks, True))
    replies = iter([""] * len(asks))
    again = prep.run("Stripe", ask=lambda _q: next(replies))
    assert all(i["done"] for i in again["items"] if not i["auto"])


def test_tests_read_only_the_base_knowledge_set(monkeypatch):
    """conftest sets CAREERBREW_EXTRA_KNOWLEDGE=0, so a checkout with batch files and one without give the same result."""
    assert set(interview.companies()) == {"amazon", "datadog", "gitlab", "google", "stripe"}
    assert interview.patterns()["companies"] == 5
    base = set(interview.sources())
    monkeypatch.delenv("CAREERBREW_EXTRA_KNOWLEDGE")
    assert set(interview.companies()) >= {"amazon", "datadog", "gitlab", "google", "stripe"} and set(interview.sources()) >= base


def test_batch_files_load_from_their_own_folder(tmp_path, monkeypatch):
    from careerbrew import paths

    root = tmp_path / "repo"
    for folder, cid in (("companies", "base-co"), ("companies-extra", "batch-co")):
        (root / "knowledge" / folder).mkdir(parents=True)
        (root / "knowledge" / folder / f"{cid}.yaml").write_text(f"id: {cid}\nname: {cid}\nloop:\n  - {{step: behavioral, notes: n, sources: [s]}}\nsources: [s]\n")
    (root / "knowledge" / "sources.yaml").write_text("sources:\n  s: {title: Base, url: 'https://example.com/a'}\n")
    (root / "knowledge" / "sources-extra.yaml").write_text("sources:\n  t: {title: Batch, url: 'https://example.com/b'}\n")
    monkeypatch.setattr(paths, "REPO_ROOT", root)
    assert set(interview.companies()) == {"base-co"} and set(interview.sources()) == {"s"}
    monkeypatch.setenv("CAREERBREW_EXTRA_KNOWLEDGE", "1")
    assert set(interview.companies()) == {"base-co", "batch-co"} and set(interview.sources()) == {"s", "t"}


def test_every_batch_file_loads_when_the_batch_set_is_on(monkeypatch):
    """Runs only in a checkout that has the batch folder; the public copy has none."""
    from careerbrew import paths

    files = sorted((paths.REPO_ROOT / "knowledge" / "companies-extra").glob("*.yaml"))
    if not files:
        pytest.skip("no batch company files in this checkout")
    monkeypatch.setenv("CAREERBREW_EXTRA_KNOWLEDGE", "1")
    known = set(interview.sources())
    for f in files:
        info = interview.loop_for(f.stem)
        assert info["known"] and info["loop"], f.stem
        assert {*info["sources"], *(s for step in info["loop"] for s in step["company_sources"])} <= known, f.stem
        assert info["company"] in interview.render(info), f.stem


def _load_script(name):
    import importlib.util
    from pathlib import Path

    path = Path(__file__).resolve().parents[2] / "scripts" / name
    spec = importlib.util.spec_from_file_location(name.replace("-", "_").removesuffix(".py"), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _research_root(tmp_path, with_batch):
    import yaml

    src = lambda n: {"title": f"Source {n}", "url": f"https://example.com/{n}"}  # noqa: E731
    step = {"name": "Behavioral", "description": "d", "evaluated": ["a", "b", "c"], "tips": ["t1", "t2", "t3"], "traps": ["x1", "x2", "x3"], "sources": [src("step")]}
    base = {
        "meta": {"researched_on": "2026-01-01"},
        "steps": {"behavioral": step},
        "claims": [{"claim_id": c, "sources": [src(c)]} for refs in ([r for _i, _t, rs in _load_script("import-research.py").RULES for r in rs],) for c in sorted(set(refs))],
        "companies": {"base-co": {"name": "Base Co", "loop": [{"step": "behavioral"}], "sources": [src("base"), src("shared")]}},
    }
    research = tmp_path / "knowledge" / "research"
    research.mkdir(parents=True, exist_ok=True)
    (research / "interview-research.yaml").write_text(yaml.safe_dump(base))
    batch = research / "interview-research-9z.yaml"
    if with_batch:
        batch.write_text(yaml.safe_dump({"meta": {"researched_on": "2026-02-02"}, "companies": {"batch-co": {"name": "Batch Co", "loop": [{"step": "behavioral"}], "sources": [src("batch"), src("shared")]}}}))
    else:
        batch.unlink(missing_ok=True)
    return tmp_path


def test_import_research_keeps_batch_output_apart_and_base_output_frozen(tmp_path, monkeypatch, capsys):
    """The base files (sources.yaml, companies/) come out the same bytes with or without a batch file."""
    module = _load_script("import-research.py")
    monkeypatch.setattr(module, "ROOT", _research_root(tmp_path, with_batch=False))
    module.main()
    knowledge = tmp_path / "knowledge"
    frozen = {f: (knowledge / f).read_bytes() for f in ("sources.yaml", "companies/base-co.yaml", "interview/steps.yaml", "strategy.yaml")}
    assert not (knowledge / "sources-extra.yaml").exists() and not (knowledge / "companies-extra").exists()

    _research_root(tmp_path, with_batch=True)
    module.main()
    assert {f: (knowledge / f).read_bytes() for f in frozen} == frozen, "a batch file never changes a base file, a base id or a base checked date"
    assert [f.name for f in (knowledge / "companies").glob("*.yaml")] == ["base-co.yaml"]
    assert [f.name for f in (knowledge / "companies-extra").glob("*.yaml")] == ["batch-co.yaml"]
    extra = yaml_load(knowledge / "sources-extra.yaml")["sources"]
    assert [s["url"] for s in extra.values()] == ["https://example.com/batch"], "only what the batch alone cites; the shared source stays a base id"
    assert not set(extra) & set(yaml_load(knowledge / "sources.yaml")["sources"])
    batch_co = yaml_load(knowledge / "companies-extra" / "batch-co.yaml")
    assert set(batch_co["sources"]) - set(extra) <= set(yaml_load(knowledge / "sources.yaml")["sources"]) and batch_co["checked"] == "2026-02-02"

    _research_root(tmp_path, with_batch=False)
    module.main()
    assert not (knowledge / "sources-extra.yaml").exists(), "removed when no batch cites anything"
    capsys.readouterr()


def yaml_load(path):
    import yaml

    return yaml.safe_load(path.read_text(encoding="utf-8"))


def test_check_sources_reads_the_batch_folder_unless_told_not_to(tmp_path, monkeypatch, capsys):
    import shutil

    from careerbrew import paths

    for folder in ("knowledge/interview", "knowledge/companies", "config"):
        shutil.copytree(paths.REPO_ROOT / folder, tmp_path / folder)
    for name in ("sources.yaml", "strategy.yaml"):
        shutil.copy(paths.REPO_ROOT / "knowledge" / name, tmp_path / "knowledge" / name)
    (tmp_path / "knowledge" / "companies-extra").mkdir()
    (tmp_path / "knowledge" / "companies-extra" / "bad.yaml").write_text("id: bad\nname: Bad Co\nexamplecity_presence: {value: true}\nloop:\n  - {step: behavioral, notes: n, sources: []}\nsources: []\n")
    module = _load_script("check-sources.py")
    monkeypatch.setattr(module, "ROOT", tmp_path)
    monkeypatch.setattr(module, "problems", [])
    assert module.main() == 0 and "5 companies, 0 problem(s)" in capsys.readouterr().out
    monkeypatch.setenv("CAREERBREW_EXTRA_KNOWLEDGE", "1")
    monkeypatch.setattr(module, "problems", [])
    assert module.main() == 1
    out = capsys.readouterr().out
    assert "6 companies" in out and "company bad loop 1: no sources" in out and "company bad examplecity_presence: no sources" in out
