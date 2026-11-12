import pytest
import yaml

from careerbrew import store
from careerbrew.cli import main
from careerbrew.jobs import criteria, platforms, sources

from .conftest import FIXTURES


@pytest.fixture
def preset(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_ADZ_ID", "id-x")
    monkeypatch.setenv("CB_ADZ_KEY", "key-x")
    (home / "jobs").mkdir(parents=True, exist_ok=True)
    entries = platforms.REMOTE_PRESET["sources"][:-1] + [{"kind": "adzuna", "country": "gb", "what": "software engineer", "where": "Example City", "app_id": "env:CB_ADZ_ID", "app_key": "env:CB_ADZ_KEY"}]
    (home / "jobs" / "sources.yaml").write_text(yaml.safe_dump({"sources": entries}))
    (home / "jobs" / "criteria.yaml").write_text(yaml.safe_dump({**platforms.REMOTE_CRITERIA, "min_seniority": 3}))
    return home


def test_every_platform_normalizes_with_credit_and_link_back(preset):
    ps = {p.id: p for p in sources.scan()}
    assert {"remoteok:901", "remotive:2000002", "himalayas:senior-platform-engineer", "hn:49922600", "wwr:pineco-senior-full-stack-engineer", "adzuna:4321"} <= set(ps)
    assert ps["remoteok:901"].source_name == "Remote OK" and ps["remoteok:901"].url == "https://remoteok.com/remote-jobs/901"
    assert ps["hn:49922600"].company == "LakeCo" and ps["hn:49922600"].region == "global" and ps["hn:49922600"].salary_max == 180000
    assert ps["himalayas:senior-platform-engineer"].region == "global" and ps["himalayas:senior-platform-engineer"].salary_min == 160000
    assert ps["wwr:pineco-senior-full-stack-engineer"].company == "PineCo" and ps["wwr:pineco-senior-full-stack-engineer"].region == "global"
    assert ps["adzuna:4321"].location.startswith("Example City") and ps["adzuna:4321"].source_name == "The Adzuna API"
    assert "hn:49922601" not in ps, "HN posts that never mention remote are skipped"


def test_preset_criteria_drop_us_only_europe_only_and_non_engineering(preset):
    kept, rejected = criteria.apply(sources.scan())
    companies = {p.company for p in kept}
    assert {"AcmeCo", "NorthCo", "ZzPlatformCo", "LakeCo", "PineCo", "CedarCo"} <= companies
    assert "UsOnlyCo" not in companies and "EuroCo" not in companies and "DesignCo" not in companies
    assert rejected["region"] == 2


def test_region_of():
    assert sources.region_of("Remote - US") == "us"
    assert sources.region_of("Example City, Example Region") == "", "no city label is built in"
    assert sources.region_of("Anywhere in the World") == "global"
    assert sources.region_of("strong interest in testing") == ""
    # every built-in label, on invented location text
    assert sources.region_of("Remote (EMEA), Example City") == "europe"
    assert sources.region_of("APAC hub, Example City") == "asia"
    assert sources.region_of("North America, Example Region") == "americas"
    # each US zone reads as americas by its long name; the eastern short name still does
    for zone in ("EST", "Eastern Time", "Central Time", "Mountain Time", "Pacific Time"):
        assert sources.region_of(f"Remote ({zone})") == "americas", zone
    # a zone word must not pull a posting from another part of the world into americas
    for text, label in (
        ("Remote, Asia Pacific time zones", "asia"),
        ("Remote, Asia-Pacific Time Zone", "asia"),
        ("Remote (APAC, CST UTC+8)", "asia"),
        ("Remote EMEA or US Pacific Time", "europe"),
        ("Remote - China (CST)", "global"),
        ("Remote, Philippines (PST)", "global"),
        ("Remote, Australian Central Time", "global"),
        ("Worldwide, 4h overlap with PST", "global"),
    ):
        assert sources.region_of(text) == label, text
    assert sources.region_of("Remote, US") == "us"
    assert sources.region_of("Hybrid, Example City") == "", "a plain city has no region label"
    assert {label for label, _words in sources.REGIONS} >= {"us", "americas", "europe", "asia", "global"}


def test_adzuna_cache_key_never_holds_the_api_key(preset):
    sources.scan()
    for f in (preset / "cache").glob("*.json"):
        assert "key-x" not in store.read_text(f)


def test_preset_command_and_credits_in_list(home, monkeypatch, capsys):
    assert main(["jobs", "preset"]) == 0
    data = store.read_yaml(home / "jobs" / "sources.yaml")
    assert {s["kind"] for s in data["sources"]} == set(platforms.KINDS)
    assert main(["jobs", "preset"]) == 1, "never overwrites without --force"
    out = capsys.readouterr().out
    assert "careerbrew secret set careerbrew.adzuna.id" in out


def test_unknown_location_is_not_rejected(home):
    from careerbrew.jobs.sources import Posting

    p = Posting(id="x", company="C", title="Senior Software Engineer", location="", remote=False, url="", description="", seniority=3)
    assert criteria.reject_reason(p, {**criteria.DEFAULTS, "locations_any": ["Example City"]}) is None


def test_adzuna_without_country_is_skipped_with_the_fix(preset):
    """A source written before `country` was required has none: no silent default country, a clear skip line instead."""
    data = store.read_yaml(preset / "jobs" / "sources.yaml")
    for entry in data["sources"]:
        entry.pop("country", None)
    (preset / "jobs" / "sources.yaml").write_text(yaml.safe_dump(data))
    lines: list[str] = []
    ps = {p.id for p in sources.scan(on_skip=lines.append)}
    assert len(lines) == 1 and "needs `country`" in lines[0] and "careerbrew data edit jobs/sources.yaml" in lines[0]
    assert "adzuna:4321" not in ps and "remoteok:901" in ps, "the other sources still complete"
    with pytest.raises(platforms.SourceSettingError, match="country"):
        platforms.fetch({"kind": "adzuna", "what": "x", "app_id": "env:CB_ADZ_ID", "app_key": "env:CB_ADZ_KEY"})


def test_preset_picks_no_adzuna_country():
    assert [s.get("country") for s in platforms.REMOTE_PRESET["sources"] if s["kind"] == "adzuna"] == [None]
